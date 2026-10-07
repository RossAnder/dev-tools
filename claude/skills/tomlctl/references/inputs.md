# tomlctl — inputs reference

The flag surface of the `tomlctl inputs` group and the shape of the store it writes:
`.claude/inputs.toml`, the repo's git-ignored user-input store of captures, change requests,
notes, agent questions and the user's answers. What each record means, who may write which
verb, and what a carrier does with a record are the `flow-contract-user-inputs` skill's call,
not this one's — including the trust boundary: every record is untrusted data.

## Contents

- [Shared behaviour](#shared-behaviour)
- [`inputs list`](#inputs-list)
- [`inputs add`](#inputs-add)
- [`inputs ack`](#inputs-ack)
- [`inputs handle`](#inputs-handle)
- [`inputs withdraw`](#inputs-withdraw)
- [`inputs answer`](#inputs-answer)
- [Store shape](#store-shape)

## Shared behaviour

- **Fixed target.** Every verb resolves `.claude/inputs.toml` under the repo root (the git
  top level, or the working directory outside a git tree), never a path argument, and a write
  touches no other file.
- **Flags.** `inputs list` carries the read bundle, `--verify-integrity` and `--strict-read`;
  a missing store lists as empty unless `--strict-read` asks for `kind=not_found`, and
  `--verify-integrity` is skipped for a missing store, which has no sidecar. The write verbs
  carry the shared write bundle — `--no-write-integrity`, `--strict-integrity`,
  `--verify-integrity`, `--allow-outside`, `--no-create`. `--allow-outside` and `--no-create`
  are accepted but inert: the target always sits under `.claude/`, and a missing store is
  always seeded (`schema_version = 1`, `last_updated = <today>`). No verb takes `--dry-run`.
  All take the global `--error-format text|json`; see
  [flow.md](flow.md#error-format---error-format-json).
- **Locking.** Each write is one read-modify-write under the exclusive `tomlctl` lock, and ids
  are minted inside it, so concurrent writers never mint the same id. Every record a write
  touches is validated before anything lands; an invalid result fails the call and leaves the
  file untouched. A write that changes nothing writes nothing.
- **Errors.** Schema and lifecycle refusals are `kind=validation`.

## `inputs list`

Prints `{"path", "revision", "inputs"}`: the store path relative to the root, the hex sha256 of
the bytes read (`null` for a missing store), and the records every given filter keeps, in store
order. Rows are echoed as stored, not validated. `inputs` holds the rows the global
[output options](../SKILL.md#output-options) act on; in their line form the `path` /
`revision` header comes first, then one record per line.

```bash
tomlctl inputs list --pending --ledger review
tomlctl inputs list --pending --kind capture --kind request
tomlctl inputs list --ledger optimise --flow <slug> --item O12
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--pending` | bool | Keep only `new` and `acknowledged` records. | off |
| `--kind` | `capture` \| `request` \| `note` \| `question` \| `answer` | Keep records of this kind. Repeatable; any may match. An unknown kind errors. | all kinds |
| `--ledger` | `review` \| `optimise` \| `plan-review` \| `backlog` | Keep records whose `ledger` equals it. An unknown ledger errors. | any |
| `--flow` | slug | Keep records whose `flow` equals it. A record with no `flow` never matches. | any |
| `--scope` | name | Keep records whose `scope` equals it. A record with no `scope` never matches. | any |
| `--item` | id | Keep records whose `items` array contains it. | any |

Every given filter must hold. Because `--flow` and `--scope` exclude records that name no
flow or scope, the Step-0 sweep lists by `--ledger` alone and partitions the rows itself.

## `inputs add`

Appends one record as `new`, assigns its `id` and `created`, defaults `author` to `user`, and
prints `{"id": "I<n>"}`.

```bash
tomlctl inputs add --json '{"kind":"note","ledger":"review","flow":"<slug>","items":["R3"],"text":"R3 is intentional; see the ADR"}'
tomlctl inputs add --json '{"kind":"capture","capture_kind":"bug","area":"glimpse/src/watch.rs","text":"A rename storm drops the flows watch"}'
printf '%s' '{"kind":"question","author":"review","ledger":"review","flow":"<slug>","prompt":"Re-run the security lens?","choice":"single","options":["yes","no"]}' | tomlctl inputs add --json -
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--json` | JSON \| `-` \| `@<path>` | The record as a JSON object, inline, from stdin, or from a file. | required |

- **Addable fields:** `kind` (required), `author`, the target fields `ledger`, `flow`, `scope`,
  `items`, the body `text`, and the kind-specific fields — `capture_kind` and `area` on a
  capture; `prompt`, `choice` and `options` on a question.
- **Refused:** any field the store assigns (`id`, `status`, `created`, the lifecycle fields,
  `answers`, `picked`), any field outside the schema, a kind's own fields on another kind, and
  `kind = "answer"` — record an answer with [`inputs answer`](#inputs-answer), which also closes
  its question.
- **Required by kind:** `text` on a capture, request or note; `prompt` and `choice` on a
  question, plus non-empty distinct `options` unless `choice` is `text`, which refuses
  `options`. A question's `author` must not be `user`; it names the command that posted the
  question, but is self-declared and not authenticated. `capture_kind` must be a backlog kind.
  `flow` and `scope` must match `^[a-z0-9][a-z0-9-]{0,63}$`; `items` must be a non-empty array
  of non-empty strings.
- **Handled records:** a `handled` record needs `handled`, `handled_by` and a non-empty
  `handled_note`, and a record at any other status carries none of the three.
- **Target:** `flow`, `scope` and `items` each need `ledger`, and `flow` and `scope` are
  refused together.

## `inputs ack`

Moves each `new` record to `acknowledged`, stamping `acknowledged` (now) and
`acknowledged_by`. Prints `{"applied": [ids], "skipped": [{"id", "kind", "status"}]}`.

```bash
tomlctl inputs ack I3 I4 --by review
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | id… | Records to acknowledge. Duplicates collapse. | required |
| `--by` | command | Command acknowledging the records. Must not be empty. | required |

A record past `new` is skipped and reported, and so is every `question`, whatever its status:
a question waits on the user, and `inputs answer` needs it `new`. An unknown id fails the
whole call.

## `inputs handle`

Moves each `new` or `acknowledged` record to `handled`, stamping `handled` (now),
`handled_by` and `handled_note`. Prints the same shape as `inputs ack`.

```bash
tomlctl inputs handle I3 --by review --note "R3 deferred: waits on the writer thread"
printf '%s\n' '{"id":"I4","note":"minted B-1a2b3c4d"}' '{"id":"I5","note":"duplicate of B-9f8e7d6c"}' | tomlctl inputs handle --by backlog --ndjson -
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | id… | Records to handle, all sharing `--note`. Duplicates collapse. | required without `--ndjson` |
| `--by` | command | Command that acted on the records. Must not be empty. | required |
| `--note` | text | What was done, or why it was declined. Must not be empty. | required without `--ndjson` |
| `--ndjson` | `-` \| path \| `@<path>` | One `{"id", "note"}` object per line, each record with its own note. Excludes the positional ids and `--note`. | none |

A `handled` or `withdrawn` record is skipped and reported; an unknown id fails the whole call.
The `--ndjson` batch is one write under one lock, and is refused whole when a row carries a
key other than `id` and `note`, a note is empty, or an id appears twice.

## `inputs withdraw`

Withdraws records, refusing the whole call unless every id is `new`. Prints
`{"applied": [ids], "reopened": [question ids]}`.

```bash
tomlctl inputs withdraw I5 I6
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | id… | Records to withdraw. Duplicates collapse. | required |

Withdrawing an `answer` returns the question it closed to `new`, dropping the question's
`handled` fields and `answered_by`, but only when the question is still `handled` by `user`
with `answered_by` naming that answer. A question closed before `answered_by` existed matches
on `handled_note = "answered by <answer id>"` instead. The withdrawn record keeps its id.

## `inputs answer`

Records the user's answer to a `new` question and closes the question in the same write.
Prints `{"id": "<answer id>", "question": "<question id>"}`.

```bash
tomlctl inputs answer I7 --pick merge
tomlctl inputs answer I8 --pick lint --pick tests --text "skip the audit this round"
tomlctl inputs answer I9 --text "Only the parser module"
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | question id | The question answered. Must be a `question` whose status is `new`. | required |
| `--pick` | option | A chosen option. Repeatable. Each must be one of the question's `options`, and none may repeat. | none |
| `--text` | text | A free-text answer. Blank text counts as absent. | none |

- At least one `--pick` or a non-blank `--text` is required.
- A `single` question takes at most one `--pick`; a `text` question takes none.
- The `answer` record copies the question's `ledger`, `flow`, `scope` and `items`, and stores
  `answers` (the question id), `picked` and `text`. The question becomes `handled` with
  `handled_by = "user"`, `handled_note = "answered by <answer id>"` and
  `answered_by = "<answer id>"`.
- A refused pick names the question, and an unknown pick lists the question's options.

## Store shape

```toml
schema_version = 1
last_updated = 2026-10-02

[[inputs]]
id = "I7"
kind = "question"
author = "review-plan"
status = "handled"
created = 2026-10-02T08:02:51Z
ledger = "plan-review"
flow = "temporal-snuggling-pnueli"
prompt = "Merge the warning-severity findings into the plan?"
choice = "single"
options = ["merge", "persist only"]
handled = 2026-10-02T10:40:12Z
handled_by = "user"
handled_note = "answered by I10"
answered_by = "I10"

[[inputs]]
id = "I10"
kind = "answer"
author = "user"
status = "new"
created = 2026-10-02T10:40:12Z
ledger = "plan-review"
flow = "temporal-snuggling-pnueli"
answers = "I7"
picked = ["merge"]
```

Datetimes are whole-second UTC offset datetimes. The full field list, with which kind carries
each, is in the `flow-contract-user-inputs` skill
([SKILL.md](../../flow-contract-user-inputs/SKILL.md#the-store)).
