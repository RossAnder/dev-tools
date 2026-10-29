# tomlctl — backlog pickup queue

The `tier` field, the focus queue, and the two verbs that read and maintain it. The rest of the
`backlog` group is in [backlog.md](backlog.md); `tier` is set with `backlog edit` ([backlog-edit.md](backlog-edit.md)).

## Contents

- [The `tier` field](#the-tier-field)
- [Queue semantics](#queue-semantics)
- [`backlog next`](#backlog-next)
- [`backlog focus`](#backlog-focus)

## The `tier` field

`tier` says who may pick a row up: `lite`, `deep` or `human`. Any other value is refused with
`kind=validation`. It is optional, survives every status transition, and is set with
`backlog edit --tier` (or in a `backlog add` payload) and removed with `--clear-tier`.

The strict filter is the global `--where tier=<T>`, which keeps only rows tiered `T`. `backlog next --tier T` is the lenient form: it also keeps untiered rows.

Captures stamp `created` and `last_seen` as UTC `Z` datetimes, so same-day rows order. A bare-date `--where created=<date>` matches every row stamped that day on the query-engine verbs (`backlog list`, `items list`). It does not apply to `backlog next`, whose output is a JSON row report where a date stays a plain string.

## Queue semantics

The queue is the live (`open` or `promoted`) rows carrying `focus`, ordered by `(focus, id)`. A row's position (1..N) is its index in that order, computed at read time and never stored; gaps in `focus` are allowed. `focus` is a TOML integer of at least 1, and is refused on a `dismissed` or `resolved` row. Resolving or dismissing a row drops its `focus`; promoting keeps it.

`focus` is owned by `backlog focus`: `backlog add` drops it from a payload and `backlog edit` refuses it.

## `backlog next`

Prints the rows an agent should pick up next.

```bash
tomlctl backlog next --tier lite --fill
tomlctl backlog next --tier deep
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--tier` | `lite`, `deep`, `human` | Drop rows whose `tier` is set to another tier; untiered rows pass. | all tiers |
| `--fill` | — | Append every unqueued `open` row, oldest first by `created`. | off |
| `--verify-integrity` | — | As on `show`. | off |
| `--strict-read` | — | As on `show`. | off |

The output is a bare array of full stored rows. First come the `open` queued rows in queue order, each with an added `position` (its position in the whole live queue); then, with `--fill`, the unqueued `open` rows with no `position`. `promoted` rows never appear.

## `backlog focus`

Maintains the queue in one write under the store lock.

```bash
tomlctl backlog focus B-a1b2c3d4,B-1a2b3c4d
tomlctl backlog focus B-a1b2c3d4 --at 1
tomlctl backlog focus B-a1b2c3d4 --remove --dry-run
tomlctl backlog focus B-a1b2c3d4 --up
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids, comma- or space-separated | Rows to act on. `--up` and `--down` take exactly one. | — |
| `--at` | K | Queue position (1-based) the new rows enter at; later rows shift down. Past the end clamps to the bottom; 0 is refused. | bottom |
| `--remove` | — | Drop the rows from the queue. The gap stays; nothing renumbers. | off |
| `--up` | — | Swap the one row with its upper neighbour. | off |
| `--down` | — | Swap the one row with its lower neighbour. | off |
| `--dry-run` | — | Emit the envelope with `dry_run: true`; touch neither file nor sidecar. | off |

At most one of `--at`, `--remove`, `--up`, `--down` per call. Adding an already queued row is refused (move it instead). When the queue holds a duplicate `focus` value it is first renumbered 1..N, and those rows appear in `changed`.

```json
{"ok":true,"applied":["B-a1b2c3d4"],"skipped_stale":[],"changed":[{"id":"B-a1b2c3d4","before":null,"after":3}],"queue":["B-1a2b3c4d","B-a1b2c3d4"]}
```

`changed` lists every row whose `focus` moved, shifted neighbours included; `queue` is the resulting queue's ids in order.
