# tomlctl — backlog recipes

How to read, search, batch-triage and edit `.claude/backlog.toml` rows through the `backlog`
group instead of the raw file. Flag tables are in [backlog.md](backlog.md).

Never `grep`, `sed` or `cat` `.claude/backlog.toml`, and never pipe `tomlctl get
.claude/backlog.toml backlog` into `jq` to find a row: rows run to twenty-odd lines, so a
`grep -A<n>` cuts some and runs into the next, and the verbs below already return exactly the
row. Output shapes, so no fallback chain is needed: `list` prints a bare array of rows (never
`{"items":…}`); `show` prints `{item, evidence, neighbours}` for one id and an array of those for
several, so its fields are `item.<field>`; `check` and `add-many` are row reports whose rows are
`candidates` / `results` / `rows`. Fields are named `summary` and `context` — there is no
`title`, `description`, `detail` or `severity`.

```bash
# Read one or several rows. A real newline inside $'…' splits the template; a literal \n does not.
tomlctl backlog show B-a1b2c3d4,B-1a2b3c4d --template $'{item.id} [{item.status}] {item.summary}\n  ctx: {item.context}'
tomlctl backlog show B-a1b2c3d4 --get item.context
# Survey the live rows as one line each, in place of `list | jq -r '.[] | "\(.id) …"'`:
tomlctl backlog list --live --template '{id} [{kind}/{status}] {area} :: {summary:120}'
# Bucket counts, in place of `jq group_by`:
tomlctl backlog list --count-by status
# Search. --where-contains is case-sensitive; --where-regex needs (?i-u) for case folding.
tomlctl backlog list --live --where-regex 'summary=(?i-u)pacing|catcher' --template '{id} {summary}'
# Terminal rows resolved this month:
tomlctl backlog list --status resolved --where-gte resolved=@date:2026-10-01 --template '{id} {resolution:160}'
```

`--template` already projects, so it refuses `--select`; drop the `--select`. Each
`--where-*` predicate is ANDed, so a search over `summary` *or* `context` takes two calls.

**Several items, each with its own resolution text.** `triage` writes one companion text per
call, so a batch with distinct wording is one call per id, each text staged with the Write tool
as its own file and guarded so a row someone else already moved is left alone:

```bash
for id in B-a1b2c3d4 B-1a2b3c4d; do tomlctl backlog triage "$id" --resolve --resolution-file ".claude/_res/$id.txt" --expect-status open -q || echo "FAIL $id"; done
```

Ids that share one text go in a single call: `triage B-a1b2c3d4 B-1a2b3c4d --resolve --resolution-file <path>`.

**Editing a stored row.** The group has no edit verb. `context`, `tags`, `evidence` and
`origin` can be patched by id through the generic ledger verb, which keeps `dedup_id`:

```bash
tomlctl items update .claude/backlog.toml B-a1b2c3d4 --array backlog --json '@<staged-file>'
```

Patch nothing else that way. `items update` and `set` run none of the backlog validator: a
`status` written there skips its date/companion pair, and a changed `summary`, `kind` or `area`
leaves the id and fingerprint describing the old text, so `check` stops answering `duplicate`
for it. A rewording is a new row plus `relate <new> --to <old> --as supersedes`. Never address a
row by array index (`set .claude/backlog.toml backlog.63.context …`) — an index found by
`grep -n` shifts with every capture and compaction.
