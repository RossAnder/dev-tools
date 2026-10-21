# tomlctl — backlog recipes

How to read, search, batch-triage and edit `.claude/backlog.toml` rows through the `backlog`
group instead of the raw file. Flag tables are in [backlog.md](backlog.md); `get`, `edit` and
the `triage --ndjson` row shape are in [backlog-edit.md](backlog-edit.md).

Never `grep`, `sed` or `cat` `.claude/backlog.toml`, and never pipe `tomlctl get
.claude/backlog.toml backlog` into `jq` to find a row: rows run to twenty-odd lines, so a
`grep -A<n>` cuts some and runs into the next, and the verbs below already return exactly the
row. Output shapes, so no fallback chain is needed: `list` and `get` print a bare row or array of
rows (never `{"items":…}`); `show` prints `{item, evidence, neighbours}` for one id and an array
of those for several, so its fields are `item.<field>`; `check` and `add-many` are row reports
whose rows are `candidates` / `results` / `rows`. Fields are named `summary` and `context` —
there is no `title`, `description`, `detail` or `severity`.

## Read

```bash
# The stored row alone, or one field bare.
tomlctl backlog get B-a1b2c3d4,B-1a2b3c4d
tomlctl backlog get B-a1b2c3d4 --get summary
# Prose fields read as blocks rather than escaped JSON strings.
tomlctl backlog get B-a1b2c3d4 --text-view
# A template splits lines on `\n` (`\t` and `\\` work too) inside plain single quotes.
tomlctl backlog get B-a1b2c3d4,B-1a2b3c4d --template '{id} [{status}] {summary}\n  ctx: {context}'
# With neighbours and evidence; the row sits under `item`.
tomlctl backlog show B-a1b2c3d4 --get neighbours
# Survey the live rows as one line each.
tomlctl backlog list --live --template '{id} [{kind}/{status}] {area} :: {summary:120}'
# Bucket counts.
tomlctl backlog list --count-by status
# Terminal rows resolved this month:
tomlctl backlog list --status resolved --where-gte resolved=@date:2026-10-01 --template '{id} {resolution:160}'
```

`--select` is accepted beside `--template` when every template path is covered by a select path.

## Search

`--grep` is one case-insensitive regex over the default fields `summary`, `context`, `area`,
`tags`, `resolution`, `dismiss_reason` and `reopen_rationale`. `--grep-field` overrides the
field set and repeats, one field per occurrence (a comma is not split), and the match ANDs with
every `--where*` predicate.

```bash
tomlctl backlog list --grep 'pacing|catcher' --template '{id} {summary}'
tomlctl backlog list --live --grep 'pacing' --grep-field summary --grep-field context --template '{id} {summary}'
tomlctl backlog list --grep 'spawn' --where area=lumina --text-view
```

## Triage several rows in one call

`triage --ndjson` takes one transition per row, so distinct wording per id and mixed
transitions need no loop. Write the record file with the Write tool, so agent-written prose
never passes through a shell argument:

```toml
[[record]]
id = "B-a1b2c3d4"
resolve = "fixed in the spawn path"
expect_status = "open"

[[record]]
id = "B-1a2b3c4d"
dismiss = "duplicate of B-a1b2c3d4"

[[record]]
id = "B-5e6f7a8b"
promote = "alpha"

[[record]]
id = "B-9c0d1e2f"
reopen = "the failure recurs on the nightly run"
```

Each record carries `id`, exactly one of `resolve`, `dismiss`, `promote` or `reopen` whose value
is the companion text, and an optional `expect_status` that leaves a row someone else already
moved untouched and reports it under `skipped_stale`.

```bash
tomlctl backlog triage --ndjson <file.toml> --dry-run
tomlctl backlog triage --ndjson <file.toml>
```

Ids that share one text go in a single flag-form call instead:
`triage B-a1b2c3d4 B-1a2b3c4d --resolve --resolution-file <path>`.

## Edit a stored row

`backlog edit` rewrites `context`, `tags`, `evidence`, `origin`, `flow` and `base_sha` by id,
keeping `dedup_id` and running the backlog validator. Stage agent-written `context` with the
Write tool and pass it by file:

```bash
tomlctl backlog edit B-a1b2c3d4 --context-file <staged-file> --add-tag pty
```

`status` and the triage fields belong to `triage`; `summary`, `kind` and `area` are not
editable, because the id and fingerprint describe the old text and `check` would stop answering
`duplicate` for it. A rewording is a new row plus `relate <new> --to <old> --as supersedes`.
Never address a row by array index (`set .claude/backlog.toml backlog.63.context …`) — an index
shifts with every capture and compaction.
