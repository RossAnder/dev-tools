# tomlctl — tasks store reference

The on-disk shape of `.claude/flows/<slug>/tasks.toml`, the `ref` rule that keys it, what the
graph engine recomputes on every read, the node cap that bounds it, and the behaviours the four
adopting carriers are entitled to rely on. The flag surface of the verbs that read it is
[tasks.md](tasks.md), and of those that write it [tasks-write.md](tasks-write.md); what the fields *mean* and which verb a carrier reaches for is the
`flow-contract-task-store` skill's job (`claude/skills/flow-contract-task-store/SKILL.md`).

## Contents

- [Store shape](#store-shape)
- [Ref derivation](#ref-derivation)
- [Derived graph products](#derived-graph-products)
- [The 512-node cap](#the-512-node-cap)
- [The snapshot read](#the-snapshot-read)
- [Frozen contracts](#frozen-contracts)

## Store shape

`.claude/flows/<slug>/tasks.toml` carries four top-level keys, an optional `minted_refs`, a
`[policy]` table, a `[[checkpoints]]` array, optional `[[import_overrides]]`, `[[file_notes]]` and `[[backlog_links]]`
arrays and an `[[items]]` array. A v2 store adds `audit`, `id_high_water`, `[[finding_links]]`,
`[[specs]]`, `[[milestones]]` and `[[changes]]`, below. Keys are written in that order and the order is stable across writes, so a
re-render is a clean diff.

| Key | Set by |
|---|---|
| `schema_version` | seeded to `1` by `flow init` and by the auto-create write path; every write re-stamps it `2` while the store holds `audit`, `id_high_water`, `finding_links`, `specs`, `milestones` or `changes`, else `1`. A binary that knows only `1` refuses a `2` store. |
| `audit` | `true` once the plan is approved; omitted otherwise. A non-boolean value is an error. Written between `last_import_refs` and `[policy]`. |
| `id_high_water` | `tasks remove`, on a store in a flow with a `plan.toml` or already holding a v2 key: the highest id a removal retired. `tasks add` and `add-many` mint past it, so an id never names a second task. A bare integer, omitted while `0`; anything but a non-negative integer is an error. Written after `audit`. |
| `last_updated` | a bare TOML date, restamped on every mutation |
| `plan_path` | `import-plan`, from the flow context under `--slug` or `--plan` under `--file` |
| `last_import_refs` | `import-plan` — the ref set the last import produced |
| `minted_refs` | `tasks add` and `add-many`, on a store with a non-empty `last_import_refs`: refs minted that no import has produced yet. Such a row is exempt from `plan/orphan-row` and absent from `removed_refs`. Every write prunes an entry the last import produced or no row holds, `tasks update --ref` re-keys one, and the key is omitted while empty, with no `schema_version` bump. Written after `last_import_refs`. |

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
cleared. `note` is otherwise authored prose the renderer writes back verbatim. `[[file_notes]]`,
`[[backlog_links]]` and the three `*_note` keys were added the same way, without a bump: absent
from every store written before them, read as empty, and omitted on write while empty.

`[[checkpoints]]`: `id` and `rationale`, in marker order. Group *membership* is not here — it
is the `checkpoint` field on each row.

`[[import_overrides]]`: `ref`, plus `files` and/or `needs` holding the plan value a
`tasks update --unlock-import-fields` patch replaced. Written by that flag, pruned by the import
that releases it and by `tasks remove` on the row it keys on; absent entirely from a store
nothing has hand-patched, so it costs an untouched store no bytes. An entry naming no field, or
keying on a `ref` no row holds, is dropped on read and on write.

`[[file_notes]]`: `ref`, `file` and `note` — the annotation a plan's `Files` entry carried
(`(new)` and the like) against the row claiming that path, so `files` itself stays a list of bare
paths for the file-claim comparisons to read. Lines nested under a bulleted path follow the first
as further `\n`-separated lines of the same `note`. Rebuilt from the plan at each import for the rows
the plan names, kept only for paths the merged row still claims, and kept whole for a row the
plan no longer names. `tasks remove` leaves a row's notes in place on a v1 flow and drops them on
a v2 store, `tasks add` and
`tasks update --ref` drop any already keyed on the `ref` they claim (a `ref` with no `[[changes]]`
row; one with a trail cannot be claimed), and a `tasks update` taking a
path off `files` drops that path's note in the same write. Omitted while empty on the
same terms as `[[import_overrides]]`.

`[[backlog_links]]`: `ref`, `closes` and `refs` — the `.claude/backlog.toml` ids a task's
`- **Backlog**:` bullet names, `refs` holding the entries the `refs` qualifier opened and `closes`
the rest. Plan-owned whole: every import rebuilds the table from the bullets, keyed by each row's
final `ref`, and a row the plan no longer names keeps no link. `tasks update --ref` re-keys the
row's entry, as it does its `[[file_notes]]` and `[[import_overrides]]` entries; on a v1 flow
`tasks remove` leaves the table alone, so until the next import an entry can key on a `ref` no row
carries, while on a v2 store it drops the row's entry, and
`tasks add` and `tasks update --ref` drop any entry already keyed on the `ref` they claim, which
is never a `ref` holding a `[[changes]]` trail. An entry with no `ref` or no id is dropped on read and on write, and the table is omitted while
empty on the same terms as `[[import_overrides]]`.

`[[finding_links]]`: `ref`, `store`, `fixes` and `settles` — the review findings a task answers,
shaped like `[[backlog_links]]` and keyed the same way. `store` is the review store's repo-relative
path; `fixes` holds the findings the task's work resolves, `settles` the ones it investigates and
proposes a fix for. Both lists are always written, an empty one as `[]`:

```toml
[[finding_links]]
ref = "fix-the-retry-bound"
store = ".claude/review-v2/auth.toml"
fixes = ["F3", "F7"]
settles = []
```

It is the one link from a task to its findings. `tasks add-many` writes it from a row's
`review_store`, `fixes` and `settles` keys; no import rebuilds it, so the table is a v2 key and
raises `schema_version` to `2`. `tasks remove` drops the row's entry, `tasks update --ref` re-keys
it, and `tasks add` and `tasks update --ref` drop any entry already keyed on the `ref` they claim.
An entry with no `ref`, or with neither `fixes` nor `settles`, is not stored: it is dropped on read
and on write, and the table is omitted while empty.

`[[specs]]`: one entry per v2 row, keyed on `ref` — `outcome`, `constraints`, `accept`, `premises`,
`means`, `design`, `precedent` and `executor`. `accept` entries carry `behaviour`, `check`, `kind`,
`positive`, `writes_build` (absent reads `true`), the optional `run_by` and `covered_by` below, and a
`probe` only `plan probes --record` writes; `kind` is absent while `check` is empty. A `premises` entry carries `claim`, `check`, `at` and the same
optional `probe`, written only by `plan probes --record --premises`; a supplied `probe` is refused,
and a re-issued premise matching in its other fields keeps its stored probe. A row's `status` is
`pending`, `in-progress`, `done`, `failed`, `deferred` or `parked`. An entry with no `ref` is dropped on read and on write.
`tasks remove` drops the spec keyed on the row, and `tasks add` and `tasks update --ref` drop any
already keyed on the `ref` they claim, which is never a `ref` holding a `[[changes]]` trail.

<a id="accept-owner"></a>**Accept owner.** `run_by` says who runs an `accept` check: `worker`, `gate`
or `covered`. `covered_by` is the exact command of one gate of the row's checkpoint milestone, set
only with `run_by = "covered"`. Each key is written only when set, so an entry with neither keeps its
bytes. The effective owner is `run_by`, else `worker` for `writes_build = false` and `gate`
otherwise; `writes_build` keeps its one meaning, whether running the check builds, tests or writes.

| Owner | Who runs the check |
|---|---|
| `worker` | The task's worker. Only this owner is probed, and only with `writes_build = false`: `run_by = "worker"` on a building check is refused at write. |
| `gate` | The milestone's gate, through [`plan emit gate-list`](plan-engine.md#plan-emit-gate-list). |
| `covered` | Nobody separately: the gate `covered_by` names already runs it. `gate-list` skips it. |

`tasks add`, `add-many` and `update` refuse an unknown `run_by`, `worker` on a building check,
`covered` without `covered_by` and `covered_by` without `covered`; `plan check` reports the same as
`plan/accept-owner-invalid`, and a `covered_by` matching no gate of the milestone as
`plan/accept-cover-unknown`. `plan view` prints the owner as a `Run by:` line under each check.

`[[milestones]]`: `id` (a `[[checkpoints]]` id), `gates`, `gate_probes`, `elaboration` (`upfront`
when absent, `deferred`, or `elaborated` once a deferred milestone's tasks exist) and
`defer_reason`. `gate_probes` is a list of `{gate, probe, verdict, at}` tables written as
`[[milestones.gate_probes]]` and omitted while empty: `gate` is one of the milestone's `gates`,
`probe` a cheap command that fails where that gate cannot pass here, and `verdict` (`holds`, `fails`
or `broken`) and `at` (the capture's head) are written only by `plan probes --record --gates`
([plan-probes.md](plan-probes.md#gate-probes)).

`[[changes]]`: the audit trail an audited store appends — `at`, `ref` (or `milestone:<id>`), `field`,
`old`, `new`, `actor` and `reason`. `field` is a row field, `*` for an added or removed row,
`spec.<field>` for a spec field, or `file_notes`, `closes` or `refs`. `old` and `new` hold a plain
scalar as it stands and a structured value as compact JSON, so a reader can parse them. The trail
outlives the row it records, so a removal stays as a tombstone, and its `ref` (a `milestone:` key
excepted) stays retired: `tasks add` suffixes past it and `tasks update --ref` refuses it. A rename
moves the row's trail to the new `ref`, freeing the old key.

`[[finding_links]]`, `[[specs]]`, `[[milestones]]` and `[[changes]]` follow `[[backlog_links]]`, in
that order, and are omitted while empty, so a v1 store round-trips byte-identical. A flow directory holding a `plan.toml` is a v2
flow: `import-plan` and `render` refuse it.

`[[items]]`: `id`, `ref`, `title`, `effort`, `status`, `checkpoint`, `phase`, `phase_depth`,
`heading_depth`, `files`, `needs`, `coupling`, `deps_note`, `action`, `detail`, `acceptance`,
`agent`, `commit`. The plan owns every field but four: `status`, `agent`, `commit` and
`coupling` survive every re-import. `files` and `needs` survive one too while an
`[[import_overrides]]` entry holds the plan value they were patched against. Per-path
annotations are not on the row — they are `[[file_notes]]` entries keyed by `ref`, as backlog
links are `[[backlog_links]]` entries.

`phase` and `phase_depth` hold the heading standing over the row in `## Tasks` and its `#` run,
`heading_depth` the row's own; absent, they read `""`, `0` and `3`. Neither `add-many` nor
`--set` reaches them — they are the plan's shape, re-read at every import.

Unknown keys are dropped — the store is tool-owned and every write re-emits it from this shape.
Bodies are LF-normalised on both read and write, and a body containing a backslash serialises as
a `'''` literal string, which golden fixtures must expect.

The array is named `items` so the generic `tomlctl items` machinery (`list`, `get`, `add`,
`add-many`, `update`, `remove`, `apply`, `next-id`, `backfill-dedup-id`) applies. Four
subcommands hardcode the review/optimise ledger schema and emit garbage against this one:
`items orphans` and `items find-duplicates`, `items sweep` (reads `sweep`, `instances`, `file`,
`symbol`) and `items clusters` (reads `file`, `instances`, `depends_on`, `enumeration`, `status`).

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

## The 512-node cap

The engine's reachability bitsets are fixed-width, so a store past **512 rows** fails graph
construction with `graph exceeds 512 tasks`. `ready`, `batches`, `closure`, `render`,
`edges --kind overlap` and `add`'s cycle check all surface that as a refusal.

`tasks check` answers differently: it reports its non-graph classes (`policy/*`,
`dag/duplicate-number`, `dag/dangling-ref`, `checkpoint/orphan-task`, `plan/orphan-row`) and
raises `dag/unbuildable` in place of the graph-derived ones, so a store above the cap exits `1`
instead of reading as a clean bill. A duplicate id or a dangling edge already accounts for a
refusal, and those findings then stand on their own.

## The snapshot read

`tasks snapshot` is one consistent read of everything a flow viewer renders: the store, its
graph products, the execution record joined to the rows, and the hook-written agent records.

```bash
tomlctl tasks snapshot --slug <slug>
```

It reads five files from one directory. `tasks.toml` is the target and must exist; its four
companions — `execution-record.toml`, `agents.toml`, `context.toml` and `plan.toml` — are resolved as
siblings of the store path, under `--file` as under `--slug`, and each reads as empty when
absent. Under `--file` the envelope's `slug` is the store's parent directory name, the value a
`--slug` read of the same store would carry.

The envelope's keys, in emission order:

| Key | Holds |
|---|---|
| `schema` | the envelope's own version, `1` |
| `revision` | the input fingerprint, below |
| `slug` | the flow slug |
| `plan_path` | the store's recorded `plan_path` |
| `flow_status` | `context.toml`'s `status`, `""` when the context is absent |
| `policy` | `checkpoints`, `max_parallel`, `commit_granularity` — the notes and `origin` are not carried |
| `running_milestone` | the `<id>` of the oldest open run in `plan.toml` whose `step` is `implement:<id>`, `""` with no such run or no `plan.toml` |
| `tasks` | one object per row in store order: `id`, `ref`, `title`, `effort`, `status`, `checkpoint`, `phase`, `files`, `needs`, `coupling`, `deps_note`, `action`, `detail`, `acceptance`, `agent`, `commit`, then the v2 keys below |
| task `spec` | the row's spec, absent when the store holds none for its `ref`: `outcome`, `constraints`, `means`, `executor`, `design`, `precedent`, `accept`, `premises`; each accept and premise entry carries `probeable`, `false` when the probe emitter would skip it, and `probe` once recorded |
| task `file_notes` | object of path to note for the row's `ref`, `{}` when none |
| task `closes`, `refs` | the backlog ids of the row's backlog link, `[]` when none |
| task `finding` | `{store, fixes, settles}` from the row's finding link, absent when none |
| `layers` | the Kahn layers `tasks batches` reports |
| `frontier` | the `tasks ready` shape, with every `in-progress` row passed as in flight |
| `edges` | the `tasks edges` list, every kind |
| `checkpoints` | per group: `id`, `rationale`, `members`, `maximal`, `valid_cut`, `commits`, `verification` |
| `record` | every execution-record entry verbatim plus `task_id`; `checkpoint` entries also carry `checkpoint_ids` |
| `agents` | every `[[agents]]` row of `agents.toml` verbatim, unknown keys included |
| `milestones` | one object per `[[milestones]]` row: `id`, `gates`, `elaboration`, `defer_reason`, `gate_probes` (`gate`, `probe`, `verdict`, `at`) |
| `changes` | the `[[changes]]` trail in stored order: `at`, `ref`, `field`, `old`, `new`, `actor`, `reason` |
| `plan` | the `plan.toml` store as JSON in its own shape, `null` when the flow has no `plan.toml` |

A record entry's `task_id` resolves its `task_ref` exactly, then through the separator-blind
matcher `--reconcile-record` uses, and is `null` when neither places it. `checkpoint` entries
carry no ref, so their `commits` are mapped to groups by commit prefix against each row's
`commit`, either side abbreviated. A group's `commits` are those, in record order; its
`verification` is the last `verification` entry whose `task_id` is a member, as
`{outcome, summary}`, or `null`.

**`revision`** is the first 16 hex characters of a SHA-256 over the five files in the order
`tasks.toml`, `execution-record.toml`, `agents.toml`, `context.toml`, `plan.toml`, each contributing its byte
length as a little-endian `u64` and then its bytes; an absent file contributes a zero length. It
is computed from the raw bytes **before** any file is parsed, so a write landing mid-read leaves
a stale revision over fresh content — which the next poll corrects — and never a fresh revision
over stale content, which a poller comparing revisions would skip. Two snapshots with equal
revisions read identical inputs.

**It is read-only.** A plain read creates nothing: no absent companion is seeded, no sidecar is
written. `--verify-integrity` checks each file that exists against its own sidecar and may leave
a lock file under `.claude/.locks/`; nothing else is written. A cycle, a dangling edge or a
store past the 512-node cap refuses the whole snapshot, as it does `ready` and `batches`.

## Frozen contracts

Behaviours downstream carriers depend on. Changing any of these is a breaking change to the
four adopting commands, not a refactor.

- **`ref` is the primary key and the upsert key.** Import matches on it, the execution record's
  `task_ref` joins on it, `last_import_refs` is a set of them.
- **Import never deletes.** A row the plan stopped naming is kept and reported in
  `removed_refs`, and `plan/orphan-row` keeps naming it until `tasks remove` retires it —
  the one verb that deletes, and never implicitly. A row `minted_refs` holds has yet to reach
  the plan, so neither names it.
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
