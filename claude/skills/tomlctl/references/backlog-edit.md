# tomlctl — backlog get, edit and batch triage

The row-only read, the free-form-field write, and the row shape of `backlog triage --ndjson`.
The rest of the `backlog` group is in [backlog.md](backlog.md).

## Contents

- [`backlog get`](#backlog-get)
- [`backlog edit`](#backlog-edit)
- [`backlog triage --ndjson`](#backlog-triage---ndjson)

## `backlog get`

The stored row alone: no `item` wrapper, no `evidence` listing, no `neighbours`. Reach for it
when only the fields matter; `backlog show` is the call that adds the neighbourhood.

```bash
tomlctl backlog get B-a1b2c3d4
tomlctl backlog get B-a1b2c3d4,B-1a2b3c4d --get status
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids, comma- or space-separated | Rows to print. At least one required. | — |
| `--verify-integrity` | — | As on `show`. | off |
| `--strict-read` | — | As on `show`. | off |

One id prints the row object; several print an array in the order given, duplicates dropped.
An unknown id fails the whole call with `kind=not_found` before anything prints.

## `backlog edit`

Rewrites the free-form fields of an existing capture. One id with field flags, or `--ndjson`
for a row per id.

```bash
tomlctl backlog edit B-a1b2c3d4 --context-file <staged-file> --add-tag pty --remove-tag wip
tomlctl backlog edit B-a1b2c3d4 --add-evidence "lumina/server/src/pty/spawn.rs:214" --dry-run
printf '%s\n' '{"id":"B-a1b2c3d4","origin":"implement"}' '{"id":"B-1a2b3c4d","tags":["ci","windows"]}' | tomlctl backlog edit --ndjson -
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | id | Row to edit. Required unless `--ndjson` is given; conflicts with it. | — |
| `--context` | text | Replace `context`. | none |
| `--context-file` | path | Replace `context` with a file's text; pass agent-written wording this way, since a backtick or `$(…)` inside a quoted shell argument executes. Conflicts with `--context`. | none |
| `--tags` | `a,b` | Replace the whole tag set. | none |
| `--add-tag` | text, repeatable | Add a tag. | none |
| `--remove-tag` | text, repeatable | Remove a tag. | none |
| `--evidence` | ref, repeatable | Replace the whole `evidence` list. | none |
| `--add-evidence` | ref, repeatable | Add an evidence ref. | none |
| `--origin` | text | Replace `origin`. | none |
| `--flow` | slug | Replace `flow`. | none |
| `--base-sha` | sha | Replace `base_sha`. | none |
| `--ndjson` | path, `@<path>` or `-` | One edit per line. Conflicts with the positional id and every field flag above. | — |
| `--dry-run` | — | Emit the envelope with `dry_run: true`; touch neither file nor sidecar. | off |

Each `--ndjson` row is `{id, context?, tags?, evidence?, origin?, flow?, base_sha?}`; a `tags`
or `evidence` array replaces the stored one.

Any other field is refused with `kind=validation`, naming the verb that owns it:

| Field | Owner |
|---|---|
| `status` and the triage fields (`promoted*`, `dismissed`, `dismiss_reason`, `resolved*`, `resolution`, `reopen_rationale`) | `backlog triage` |
| `summary`, `kind`, `area` | capture a successor with `backlog add` and `supersedes` |
| `related`, `duplicate_of`, `supersedes` | `backlog relate` |
| `created`, `last_seen`, `seen_count`, `id`, `dedup_id` | minted; never edited |

The batch is validated whole before the write and is all-or-nothing. `last_updated` is stamped
only when a row changed.

```json
{"ok":true,"path":".claude/backlog.toml","rows":[{"id":"B-a1b2c3d4","changed":true}]}
```

## `backlog triage --ndjson`

The batch form of `backlog triage`: a transition per row, so one call can resolve some rows,
dismiss others and promote a third. The flag table is in [backlog.md](backlog.md#backlog-triage).

```bash
printf '%s\n' '{"id":"B-a1b2c3d4","resolve":"fixed in the spawn path"}' '{"id":"B-1a2b3c4d","dismiss":"duplicate of B-a1b2c3d4","expect_status":"open"}' | tomlctl backlog triage --ndjson -
```

Each row is `{id, resolve|dismiss|promote|reopen: <companion>, expect_status?}` with exactly one
mode key. The companion is the resolution text, the dismiss reason, the promote target slug
or the reopen rationale. `reopen` may instead be `true` with a `rationale` key; that form is
refused beside any other mode key. A duplicate id, two mode keys or an unknown key is refused
before the write.

`--expect-status` is each row's default `expect_status`. Rows whose status has moved are left
untouched and reported under `skipped_stale`. All other rows apply in one write, or none do.
`--dry-run` is accepted only with `--ndjson`. Per-row `--external` and `--allow-closed` are
not supported. A flag-form call that names no mode is refused with `kind=validation`.

The envelope is the id-list one (`ok`, `transition`, `ids`, `path`, plus `applied` and
`skipped_stale` when expectations are given), with `transition: "mixed"` when the rows differ
and a `rows` array:

```json
{"ok":true,"transition":"mixed","ids":["B-a1b2c3d4","B-1a2b3c4d"],"path":".claude/backlog.toml",
 "rows":[{"id":"B-a1b2c3d4","transition":"resolve"},{"id":"B-1a2b3c4d","transition":"dismiss"}]}
```

A `promote` row carries the stored `to` in its `rows` entry.
