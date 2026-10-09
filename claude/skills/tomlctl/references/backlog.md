# tomlctl — backlog reference

The flag surface of the `tomlctl backlog` group and the shape of the store it writes:
`.claude/backlog.toml`, a repo-scoped capture log with a git-ignored evidence drop-box beside
it. Every op resolves the store itself — there is no file argument anywhere in the group, and
no `--json` output flag either, because every op already emits JSON on stdout. When to mint a
row and when not to is the `backlog-capture` skill's job
(`claude/skills/backlog-capture/SKILL.md`); this file is the flag table.

## Contents

- [`backlog add`](#backlog-add)
- [`backlog add-many`](#backlog-add-many)
- [`backlog check`](#backlog-check)
- [`backlog list`](#backlog-list)
- [`backlog show`](#backlog-show)
- [`backlog relate`](#backlog-relate)
- [`backlog triage`](#backlog-triage)
- [`backlog reconcile`](backlog-reconcile.md)
- [`backlog cluster`](#backlog-cluster)
- [`backlog compact`](#backlog-compact)
- [`backlog evidence dir`](#backlog-evidence-dir)
- [`backlog evidence audit`](#backlog-evidence-audit)
- [Store shape](#store-shape)
- [Id derivation](#id-derivation)
- [The `check` verdict ladder](#the-check-verdict-ladder)
- [Evidence directories](#evidence-directories)
- [Duration grammar](#duration-grammar)
- [Frozen contracts](#frozen-contracts)

Every mutating op (`add`, `add-many`, `relate`, `triage`, `reconcile`, `compact`) carries the shared write
bundle — `--allow-outside`, `--no-create`, `--no-write-integrity`, `--strict-integrity`,
`--verify-integrity` — `reconcile` included, although it writes only under `--adopt` or
`--apply`. Every read op (`check`, `list`, `show`, `cluster`, `evidence audit`,
`evidence dir`) carries the read bundle, `--verify-integrity` and `--strict-read`.
The store lives inside the `.claude/` containment guard, so none of these needs
`--allow-outside`. `backlog evidence dir` carries the read bundle but not the write one: it
reads the store to resolve the id, and the one file it writes is a non-TOML marker, so there
is no sidecar to refresh. Every op also takes the global `--error-format text|json`; see
[flow.md](flow.md#error-format---error-format-json) for the JSON envelope and its `kind`
taxonomy. The global output options ([query.md](query.md)) act on each op's report: the rows
of `add-many` (`rows`), `check` (`candidates`, or `results` under `--ndjson`) and `list`;
`--header` for a row report's header, and `--rows <field>` to read a single object's array as
rows (`backlog show <id> --rows neighbours`).

## `backlog add`

```bash
tomlctl backlog add --summary "conpty spawn intermittently fails with CreateProcessW error 5" --kind flaky-test --area lumina/server/src/pty --tag pty --context "Empty PATH entry in HKLM; set LUMINA_CLAUDE_BIN to an absolute path to work around it." --evidence "lumina/server/src/pty/spawn.rs:214" --related B-1a2b3c4d --origin implement --flow lumina-pty-hardening
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--summary` | text | Hashed, after normalisation, into the item's id. Required unless `--json` is given. | — |
| `--kind` | text | `bug`, `flaky-test`, `debt`, `direction`, `annoyance`, `question`, `other`. Free-form at the parser: an unrecognised value is coerced to `other` rather than rejected, with an advisory in the envelope's `advisories` (echoed to stderr on a terminal). | `other` |
| `--area` | repo-relative path | File or directory prefix the discovery sits under. Hashed verbatim. | empty |
| `--tag` | text, repeatable | Free-form tag. | none |
| `--evidence` | ref, repeatable | A `path:line` pointer into tracked source, or a bare filename inside the item's own evidence directory. Nothing else. | none |
| `--related` | id, repeatable | Existing backlog id this item relates to. | none |
| `--context` | text | How to work around the issue. The field that makes a later `check` hit actionable. | none |
| `--origin` | text | Command or agent that minted the row — a bare command or agent name, no leading slash. | none |
| `--flow` | slug | Flow in force at mint time. | none |
| `--base-sha` | sha | Commit the capture was made against. Conflicts with `--json`, which should carry the value in its payload. | none |
| `--auto-base-sha` | flag | Resolve `base_sha` from `git rev-parse HEAD`. Composes with `--json` — it supplies no content — and a payload's own `base_sha` wins. Silently records nothing outside a repo, on an unborn HEAD, or with no git on PATH: a capture is worth more than its provenance. | off |
| `--on-duplicate` | `bump` \| `skip` \| `fail` | Behaviour when the computed `dedup_id` is already stored. | `bump` |
| `--json` | payload or `-` | Whole-item JSON instead of the field flags; `-` reads stdin. Mutually exclusive with every field flag above — passing both errors with `kind=validation`. A key no stored row carries is refused with `kind=validation` naming it. | — |
| `--dry-run` | — | Emit the mutation plan plus the would-be envelope keys; touch neither file nor sidecar. | off |

`--on-duplicate bump` increments `seen_count`, refreshes `last_seen`, and unions `tags`,
`evidence` and `related`, leaving `summary` and `status` alone. `skip` reports the incumbent
and writes nothing — including no sidecar rewrite. `fail` errors with `kind=validation`, naming
the incumbent. No value appends a second row: one id maps to one row.

`id`, `dedup_id`, `created`, `last_seen` and `seen_count` are minted from content or from the
clock, so a `--json` payload replayed out of a `show` has those five overwritten rather than
honoured.

Envelope — one of three actions. `--dry-run` adds the would-be `action`, `id`, `dedup_id` or
`seen_count`, and `advisories` beside `"dry_run":true` and `would_change`:

```json
{"ok":true,"action":"added","id":"B-a1b2c3d4","dedup_id":"<16 hex>","created":false,"path":".claude/backlog.toml"}
{"ok":true,"action":"bumped","id":"B-a1b2c3d4","seen_count":3}
{"ok":true,"action":"skipped","id":"B-a1b2c3d4"}
```

## `backlog add-many`

The batch form of `add`: one `add --json` payload per NDJSON line, captured under one lock, one
read and one write. Reach for it whenever more than one row is being minted.

```bash
printf '%s\n' '{"summary":"…","kind":"bug","area":"src/a"}' '{"summary":"…","kind":"debt","area":"src/b"}' | tomlctl backlog add-many --ndjson - --auto-base-sha --template '{line} {action} {id}'
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--ndjson` | path, `@<path>` or `-` | One JSON object per line; `-` reads stdin. Blank lines are skipped but still counted. | — |
| `--auto-base-sha` | flag | As on `add`, for every row whose payload carries no `base_sha`. | off |
| `--on-duplicate` | `bump` \| `skip` \| `fail` | As on `add`, per row. A line colliding with a row an earlier line minted bumps it, as two `add` calls in sequence would. | `bump` |
| `--dry-run` | — | Emit the mutation plan plus `rows`; touch neither file nor sidecar. | off |

Each row runs exactly as `add --json` would — same id derivation, same advisories, same
refusal of an unknown key — and a `related` id may name a row an earlier line minted. The
batch is all-or-nothing. A malformed line, an unknown key (both `kind=validation`) or any row
`add` would refuse aborts it before the write, the error naming the 1-based line; an all-`skip`
batch writes nothing.

```json
{"ok":true,"path":".claude/backlog.toml","created":false,"added":["B-a1b2c3d4"],"bumped":["B-1a2b3c4d"],"skipped":[],
 "rows":[{"line":1,"action":"added","id":"B-a1b2c3d4","dedup_id":"<16 hex>","advisories":[]},
         {"line":2,"action":"bumped","id":"B-1a2b3c4d","seen_count":3,"advisories":[]}]}
```

Each `rows` entry carries `add`'s keys for that row. A top-level `advisories`, each entry
prefixed `line N:`, appears only when some row raised one.

## `backlog check`

Read-only. A missing store answers `novel`, so the first capture in a repo needs no setup.

```bash
tomlctl backlog check --summary "conpty spawn intermittently fails with CreateProcessW error 5" --kind flaky-test --area lumina/server/src/pty --tag pty --header --get verdict
# A summary somebody else wrote, staged to a file with the Write tool:
tomlctl backlog check --summary - --kind flaky-test --area lumina/server/src/pty --tag pty < <staged-file>
# Several probes against one store read — one JSON object per line, staged the same way:
tomlctl backlog check --ndjson <staged-file> --template '{line} {verdict} {summary:60}'
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--summary` | text or `-` | The discovery being weighed. Required unless `--ndjson` is given. `-` reads the whole of stdin as the summary, less one trailing newline — the path text somebody else wrote takes to the gate without being tokenised by a shell. Empty stdin is a `kind=validation` error, as is an `@<path>` naming an existing file (stage it and pass `-`); an `@mention` naming no file is probed as text. | — |
| `--ndjson` | path, `@<path>` or `-` | A batch of probes, one `{summary, kind, area, tags}` object per line; only `summary` is required. Conflicts with `--summary`, `--kind`, `--area` and `--tag`. | — |
| `--kind` | text | Must match the `--kind` the following `add` will use — it is hashed into the fingerprint. | `other` |
| `--area` | repo-relative path | Must match the following `add` for the same reason; also feeds the structural `related` rung. | empty |
| `--tag` | text, repeatable | Feeds the structural `related` rung only. | none |
| `--limit` | integer | The global flag: return at most N candidates. Under `--ndjson` it cuts result rows instead, and each row keeps at most 5 candidates. `verdict` sits in the header, so read it with `--header --get verdict`. | `5` |
| `--similarity-strong` | 0.0–1.0 | Char-trigram Jaccard at or above which a candidate reads as `likely-duplicate`. | `0.75` |
| `--similarity-related` | 0.0–1.0 | Word Jaccard at or above which a candidate reads as `related`. | `0.35` |

A threshold outside 0.0–1.0, or NaN, errors with `kind=validation`.

```json
{"verdict":"related","dedup_id":"<16 hex>","thresholds":{"strong":0.75,"related":0.35},
 "candidates":[{"id":"B-a1b2c3d4","summary":"…","score":0.41,"reason":"words",
                "status":"open","seen_count":2,"context":"…","evidence_files":1}]}
```

`score` is rounded to four decimals. `evidence_files` is counted off the filesystem at read
time; nothing in the store records it. A candidate carries `promoted_to` only when its row
holds one: an `in-flight` hit always does, and a `resolved`, `dismissed` or compacted row does
when it kept the claim. When the default cap of 5 cut candidates, the header adds
`limited: {"shown":5,"total":N}`.

Under `--ndjson` every line is parsed before the store is read once; a malformed line, a
missing `summary` or any other key is `kind=validation` naming the line. Each result row
carries one probe's verdict, and `limited` when its candidates were capped:

```json
{"thresholds":{"strong":0.75,"related":0.35},
 "results":[{"line":1,"summary":"…","verdict":"novel","dedup_id":"<16 hex>","candidates":[]}]}
```

## `backlog list`

`--live`, `--tag`, `--area-prefix` and `--has-evidence` narrow the `backlog` array before the
generic query engine sees it, so the whole `items list` surface — `--where-*` predicates, `--select` / `--exclude` / `--pluck`,
`--sort-by` / `--limit` / `--offset`, `--count-by` / `--group-by`, `--raw` / `--lines` /
`--ndjson` — applies on top. See [query.md](query.md) for that half.

```bash
tomlctl backlog list --open --area-prefix lumina/server --has-evidence --sort-by seen_count:desc
# The compact survey shape — one projected object per line, a bare array otherwise:
tomlctl backlog list --open --select id,kind,area,tags,summary --ndjson
# Everything still asking for work, promotion claims included:
tomlctl backlog list --live --area-prefix lumina/server --select id,status,promoted_to,summary --ndjson
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--status` | text | Exact match on `status`. | none |
| `--open` | — | Shorthand for `--status open`. | off |
| `--live` | — | Keep only live rows, `open` or `promoted`. An OR over statuses, which `--where` cannot express because its predicates AND. Conflicts with `--open` and `--status`. | off |
| `--kind` | text | Exact match on `kind`. | none |
| `--tag` | text, repeatable | Item carries TAG; repeats are ANDed. | none |
| `--area-prefix` | repo path | Matches on path-component boundaries, so `lumina/server` selects `lumina/server/pty/x.rs` but not `lumina/server-extras/y.rs`. | none |
| `--has-evidence` | — | Keep only items whose evidence directory holds files. Reads the filesystem. | off |
| `--count` | — | Emit `{"count":N}` instead of the rows. | off |

Output is the query engine's: an array of item objects, `{"count":N}` under `--count`, or a
bucket map under `--count-by`.

## `backlog show`

```bash
tomlctl backlog show B-a1b2c3d4
tomlctl backlog show B-a1b2c3d4,B-1a2b3c4d --get item.status
```

Takes ids comma- or space-separated; one prints the object below, several an array of them in
order, and an unknown id fails the whole call before anything prints. Each object holds the
stored row, its evidence listing, and its one-hop typed-edge neighbourhood in both directions:

```json
{"item":{"id":"B-a1b2c3d4","summary":"…"},
 "evidence":{"dir":".claude/backlog-evidence/B-a1b2c3d4","files":[{"name":"spawn-error-5.png","bytes":48210}]},
 "neighbours":[{"id":"B-1a2b3c4d","relation":"related","direction":"out",
                "summary":"…","status":"open","evidence":null}]}
```

`evidence` is `null` when no directory was ever created, `files: []` when a directory exists
but its contents are not in this clone, and a populated list when they are. `relation` is one
of `related`, `duplicate_of`, `supersedes`; `direction` is `out` when the subject carries the
edge and `in` when a peer points at it.

## `backlog relate`

```bash
tomlctl backlog relate B-a1b2c3d4 --to B-1a2b3c4d --as relates-to
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | id | Subject of the edge — the item that gains the field. | required |
| `--to` | id | Object of the edge. | required |
| `--as` | `relates-to` \| `duplicates` \| `supersedes` | Edge kind. | required |

`relates-to` is symmetric: both items gain the other in `related`. `duplicates` sets the
subject's `duplicate_of` and dismisses the **subject**. `supersedes` sets the subject's
`supersedes` and dismisses the **object**. The asymmetry is easy to get backwards — in each
case the item that loses is the redundant one. `--as related` is accepted as `relates-to`.

```json
{"ok":true,"relation":"relates-to","a":"B-a1b2c3d4","b":"B-1a2b3c4d","changed":true,"path":".claude/backlog.toml"}
```

`changed` is `false` when the edge was already present.

## `backlog triage`

```bash
tomlctl backlog triage B-a1b2c3d4 --promote --to <flow-slug>
tomlctl backlog triage B-a1b2c3d4 B-1a2b3c4d --promote --to GH-412 --external
tomlctl backlog triage B-a1b2c3d4 --reopen --rationale "flow <flow-slug> closed without it"
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids, comma- or space-separated | Items to transition. At least one required. | — |
| `--promote` | — | Status → `promoted`; takes `--to`. | — |
| `--dismiss` | — | Status → `dismissed`; takes `--reason` or `--reason-file`. | — |
| `--resolve` | — | Status → `resolved`; takes `--resolution` or `--resolution-file`. | — |
| `--reopen` | — | Status → `open`; **requires** `--rationale` or `--rationale-file`. | — |
| `--to` | slug or plan path | The promotion target. Must resolve to an existing flow or plan — see below. | none |
| `--external` | — | Store `--to` as `external:<REF>` without resolving it. Requires `--promote`. | off |
| `--allow-closed` | — | Accept a `--to` flow at `review` or `complete`. Requires `--promote`. | off |
| `--reason` / `--reason-file` | text / path | Companion to `--dismiss`. Mutually exclusive. Each `-file` form drops a leading BOM and one trailing newline, as `--set-file` does; pass agent-written or user-supplied wording that way, staged with the Write tool, since a backtick or `$(…)` inside a quoted shell argument executes. A fixed literal can stay inline. | none |
| `--resolution` / `--resolution-file` | text / path | Companion to `--resolve`. Mutually exclusive. | none |
| `--rationale` / `--rationale-file` | text / path | Companion to `--reopen`. Mutually exclusive. | none |
| `--expect-status` | status | Move only ids still at this status; stale ids are left untouched and reported under `skipped_stale`. | none |

The four mode flags are a required, mutually-exclusive group: exactly one per invocation.
`--rationale` (or its file form) is enforced at the parser rather than the validator because `reopen_rationale`
is the only companion an `open` item may carry, so a bare `--reopen` would write a row the
validator then rejects. Without `--expect-status` triage ignores a row's current status, so
any mode applies to any row, and re-promoting a `promoted` row moves its claim.

Each transition clears every managed field its target status does not own — see
[Store shape](#store-shape). `--resolve` and `--dismiss` keep the promotion claim
(`promoted` + `promoted_to`) as the history of where the item was sent; `--reopen` drops it,
so name the flow in the rationale. Every triage transition drops the `resolved_flow` /
`resolved_tasks` / `resolved_commits` link, which only `reconcile --apply` writes.

`--to` resolves, in order, to:

1. an `external:<text>` value — stored verbatim; `--external` is not needed and does not
   prefix it twice;
2. a slug with a `.claude/flows/<slug>/context.toml` — stored as the slug;
3. a plan path some flow's `plan_path` binds, spelled with `/` or `\`, with or without a
   leading `./`, or absolute under the repo root — stored as that flow's **slug**;
4. an existing repo-relative `.md` file no flow binds — stored as the normalised path;
5. anything else, including a path that escapes the repo root.

A flow whose `context.toml` does not parse is skipped with a stderr note, so its slug falls
through to the last case. That case is refused with `kind=not_found` unless `--external` is
given; the remedy is to bootstrap a draft seed flow (`/backlog` offers it, and the
`backlog-capture` skill defines it) or to promote as external. A flow at `review` or `complete` is refused with `kind=validation`
unless `--allow-closed` is given. `--external` stores every `--to` as `external:<REF>`, even
one that would have resolved. Both refusals come before the write, so they leave the store and
its sidecar untouched.

Under `--error-format json` both refusals carry `"arg":"to"` in the error envelope. An unknown
or compacted item id is also `kind=not_found`, but carries `"arg":"ids"`, so branch on `arg`
rather than on the message to tell a bad target from a stale id.

```json
{"ok":true,"transition":"promote","ids":["B-a1b2c3d4"],"path":".claude/backlog.toml","to":"lumina-pty-hardening"}
```

`to` appears on `--promote` only, and is the value stored — a slug where `--to` named a bound
plan path, `external:<REF>` under `--external`.

With `--expect-status` the envelope adds `applied` and `skipped_stale`, each stale entry naming
its mismatched `status` (or a missing `id`); when no id passes the guard nothing is written.

## `backlog reconcile`

`backlog reconcile` joins `promoted` rows to the tasks that close them and resolves the
finished ones. Its flags, buckets, `render_needed` duty, `--apply` writes and output shape are
in [backlog-reconcile.md](backlog-reconcile.md).

## `backlog cluster`

Read-only. Groups items into candidate work scopes across three independent views, never
blended into one score.

```bash
tomlctl backlog cluster --by area --min-size 2 --min-shared-tags 2
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--by` | `area` \| `tags` \| `relations` \| `all` | Which views to emit. | `all` |
| `--min-size` | integer | Smallest group the area view will emit; a smaller group collapses upward to a shorter path prefix. | `2` |
| `--min-shared-tags` | integer | Tags two items must share before the tags view groups them; groups then merge transitively. | `2` |
| `--per-tag` | — | Emit one tags-view group per individual tag instead of merging shared-tag groups transitively; an item carrying two tags appears in both. Ignores `--min-shared-tags`. | off |
| `--all-statuses` | — | Cluster every item rather than only `open` ones. | off |

The `relations` view is connected components over the typed edge set and consults neither
threshold. Output carries one key per requested view; a view not requested is absent, not
empty:

```json
{"area":[{"key":"lumina/server/src/pty","reason":"shared path prefix lumina/server/src/pty","size":3,
          "item_ids":["B-a1b2c3d4","B-1a2b3c4d","B-9f8e7d6c"],
          "kinds":["bug","flaky-test"],"areas":["lumina/server/src/pty"]}],
 "tags":[],"relations":[]}
```

`reason` is prose for a reader, not an enum: `shared path prefix <key>` or `no area recorded`
(area view), `share tags <key>` (tags view), `share tag <key>` (`--per-tag`), and
`linked by relates-to/duplicates/supersedes edges` (relations view). Branch on the view key
and `key`, never on `reason`.

The default tags view merges transitively, so it cannot answer "which items carry `ci`";
`backlog cluster --by tags --per-tag` does — one group per tag, still dropping groups of fewer
than two. Both forms emit under `tags`, but the default `key` is a `+`-joined tag *set*
(`"ci+windows"`) and the `--per-tag` one a single tag (`"ci"`). `--rows tags` reads either as rows.

## `backlog compact`

Folds terminal rows — `resolved` and `dismissed` — out of `[[backlog]]` into `[[compacted]]`.
Live rows, `open` and `promoted`, are never folded regardless of age: a `promoted` row is a
claim on a flow that is still owed, and folding it would make `check` answer
`previously-resolved` for undelivered work. A dead item should therefore be dismissed, and a
dead claim reopened or dismissed, rather than left to rot.

```bash
tomlctl backlog compact --older-than 90d --dry-run
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--older-than` | duration | Whole days between a row's terminal date and today. See [Duration grammar](#duration-grammar). | `90d` |
| `--dry-run` | — | Emit the plan; touch neither file nor sidecar. | off |

The comparison is **strict**: a row dated exactly `today - threshold` stays. A terminal row
whose date is missing or unreadable is left in place with a stderr note — the sweep runs
unattended, where one bad row must neither vanish nor stop the run. A sweep that folds nothing
skips the write entirely, leaving the store and its sidecar byte-identical, and a sweep that
finds no store leaves none behind.

A folded row keeps `promoted_to` and `resolved_flow` when it carried them — see the compacted
shape under [Store shape](#store-shape). Nothing here touches an evidence directory: a folded
row keeps its id and id resolution reads both arrays, so the drop-box stays reachable.

```json
{"ok":true,"compacted":4,"remaining":37,"path":".claude/backlog.toml"}
{"ok":true,"dry_run":true,"would_change":{"kind":"compact","compacted":4,"remaining":37,"ids":["B-a1b2c3d4"]}}
```

## `backlog evidence dir`

Resolves an id against the store and prints its drop-box, creating the directory and its
`.evidence` marker when absent.

```bash
tomlctl backlog evidence dir B-a1b2c3d4
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | id | Item whose directory to resolve. | required |
| `--no-create` | — | Report the directory without creating it; error if absent. | off |

Resolving rather than deriving is the whole job — see [Evidence directories](#evidence-directories).

```json
{"ok":true,"id":"B-a1b2c3d4","dir":".claude/backlog-evidence/B-a1b2c3d4","created":true,"files":[]}
```

Copy into exactly the path it printed.

## `backlog evidence audit`

Walks `.claude/backlog-evidence/` and reports every directory the store does not own, plus policy and stale-reference findings.

```bash
tomlctl backlog evidence audit --strict --max-bytes 2097152
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--strict` | — | Exit 1 on the seven failing classes below. | off |
| `--max-bytes` | integer | Oversize threshold in bytes. | `2097152` (2 MiB) |

Eleven finding classes, seven of them strict:

| Class | Meaning | `--strict` fails |
|---|---|---|
| `unowned` | A directory no stored id resolves to — usually a hand-derived path. | yes |
| `no-marker` | A drop-box holding files but no `.evidence` file, so it will not survive a fresh clone. | yes |
| `oversize` | A file past `--max-bytes`. | yes |
| `disallowed-extension` | An extension outside `png`, `jpg`, `jpeg`, `gif`, `webp`, `svg`, `txt`, `log`, `json`, `har`, `csv`, `md`, `patch`, `diff`, compared lowercased. An extensionless file lands here too. | yes |
| `referenced-missing` | A filename named in the item's `context` prose or `evidence` list is not in the populated directory. | yes |
| `stray` | A file sitting at the evidence root rather than in an item's drop-box, where no per-item ignore rule covers it. | yes |
| `sensitive-published` | A published file whose format routinely carries an `Authorization` header, a cookie or a token — `har`, `json`, `log`, `patch`, `diff`. | yes |
| `tracked` | A published file — committed or staged rather than ignored. A sensitive extension promotes it to `sensitive-published`, which is emitted instead of this class rather than alongside it. | no |
| `nested` | A subdirectory inside a drop-box. Its contents stay ignored but are not sized, classified, or matched against the item's citations. | no |
| `empty` | A drop-box holding only its marker. | no |
| `git-unavailable` | `git check-ignore` did not run, so nothing could be classified as published. | no |

`tracked`, `nested` and `empty` are advisory deliberately: a tracked file is a considered
`git add -f`, a subdirectory is a reporting gap rather than an exposure, and an empty directory
is the normal state in a fresh clone. The extension lists and the size threshold are advisory
constants that only `audit` consults — no write path enforces any of them, so the audit is the
only thing standing between a stray `.pem` and a reviewer.

```json
{"root":".claude/backlog-evidence",
 "findings":[{"class":"unowned","dir":".claude/backlog-evidence/B-deadbeef","detail":"…"},
             {"class":"oversize","dir":".claude/backlog-evidence/B-a1b2c3d4","file":"trace.log","detail":"…"}],
 "counts":{"unowned":1,"no-marker":0,"oversize":1,"disallowed-extension":0,
           "referenced-missing":0,"stray":0,"sensitive-published":0,"tracked":0,
           "nested":0,"empty":0,"git-unavailable":0}}
```

`file` is present only on file-scoped findings. Every class appears under `counts` with a zero,
so a consumer can index the map without a presence check.

## Store shape

`.claude/backlog.toml` carries `schema_version` (an integer, seeded to `1`), `last_updated` (a
TOML date, refreshed on every write) and two arrays of tables.

`[[backlog]]` — the live captures:

| Field | Set by |
|---|---|
| `id` | minted — `B-` plus the leading hex of `dedup_id` |
| `dedup_id` | minted — the content fingerprint |
| `summary`, `kind`, `area`, `tags` | the caller, at mint |
| `status` | `open` at mint, then `triage`, `reconcile --apply`, and the dismissal half of `relate` |
| `created`, `last_seen`, `seen_count` | minted from the clock; a `bump` refreshes the last two |
| `context`, `origin`, `flow` | the caller, at mint |
| `base_sha` | the caller, at mint, via `--base-sha` or `--auto-base-sha`; absent on rows minted before it existed |
| `evidence` | the caller — `path:line` pointers and bare filenames, never a directory listing |
| `related`, `duplicate_of`, `supersedes` | `add --related` and `relate` |
| `promoted`, `promoted_to` | `triage --promote` — the promotion claim |
| `dismissed`, `dismiss_reason` | `triage --dismiss`, and `relate --as duplicates` / `supersedes` |
| `resolved`, `resolution` | `triage --resolve` and `reconcile --apply` |
| `reopen_rationale` | `triage --reopen` |
| `resolved_flow`, `resolved_tasks`, `resolved_commits` | `reconcile --apply` only — the flow slug, the task refs, and the commit SHAs (an array, possibly empty) that delivered the item |

The statuses split into **live** — `open` and `promoted`, rows still asking for work — and
**terminal** — `resolved` and `dismissed`. `backlog list --live` selects the first pair, and
`compact` folds only the second.

Each non-`open` status requires its own date/companion pair: `promoted` needs `promoted` +
`promoted_to`, `dismissed` needs `dismissed` + `dismiss_reason`, `resolved` needs `resolved` +
`resolution`. `open` requires nothing and may carry `reopen_rationale`. No row may carry
another status's pair, with one exception: `resolved` and `dismissed` may keep the promotion
claim, `promoted` + `promoted_to`, as the history of where the item was sent — both halves or
neither. `open` carries no claim. The resolution link, `resolved_flow` / `resolved_tasks` /
`resolved_commits`, is allowed on `resolved` alone. Every half of that invariant is checked on
write.

`[[compacted]]` — the aged-out rows, a narrower shape: `id`, `dedup_id`, `summary`, `kind`,
`area`, `status`, `terminal_date`, `terminal_reason`, `context`, `compacted_on`, plus
`promoted_to` and `resolved_flow` when the live row carried them non-empty. The terminal
date/companion pair collapses into the single `terminal_date` / `terminal_reason` pair.
`dedup_id` and `context` are load-bearing
rather than archival — `check`'s `previously-resolved` verdict keys on the first and reports
the second, so folding a row away never loses the "we already decided this" answer.

Ids are unique across the **union** of the two arrays.

The store carries no evidence field of any kind, and the live array is named `backlog` rather
than `items` on purpose: an array named `items` under `.claude/` is the default target of
`tomlctl items add|update|apply`, whose own dedup stamping would overwrite the content-derived
`dedup_id`.

## Id derivation

An item's id is `B-` plus the leading hex of
`dedup_id = sha256(kind | area | normalise(summary))` — `kind` and `area` hashed verbatim,
`summary` through the normaliser. Widths widen 8 → 10 → 12 hex on collision, and which row
widens is decided by a **total order on `dedup_id`**: of the fingerprints sharing a prefix the
lexicographically smallest keeps the shorter id and the rest widen. Ordering by `dedup_id`
rather than by insertion order is what makes two worktrees resolve the same collision the same
way; an insertion-order tie-break hands them mirror-image assignments for the same pair.
Compacted rows count as incumbents — their ids are still taken.

Two consequences worth stating outright:

- **`check` and `add` must be given identical `--kind` and `--area`.** Change either between
  the two calls and the gate probed a different fingerprint than the mint writes: `check`
  answers `novel` and `add` lands a second row for a known issue. This is the single most
  common way to defeat the gate.
- Because the derivation is content-only, the same discovery minted from another worktree,
  branch or machine lands on the same id. Ids are stable, not allocated.

A rephrased summary is a different fingerprint unless the rephrasing folds away under
normalisation. Reuse the wording the store already has rather than improving it.

## The `check` verdict ladder

Candidates are graded against the probe on eight rungs; the first rung a row clears is its
`reason`, and the strongest reason across all candidates is the overall `verdict`. Rung order
is ladder order:

| `reason` | Rung | `verdict` |
|---|---|---|
| `dedup_id` | Exact fingerprint match on a `[[backlog]]` row that is not `promoted` | `duplicate` |
| `in-flight` | Exact fingerprint match on a `promoted` row — the item is claimed by the flow its `promoted_to` names | `in-flight` |
| `compacted` | Exact fingerprint match on an aged-out row | `previously-resolved` |
| `duplicate-id` | Two stored rows share one id | `duplicate-id` |
| `trigram` | Char-trigram Jaccard ≥ `--similarity-strong` (`0.75`) | `likely-duplicate` |
| `words` | Word Jaccard ≥ `--similarity-related` (`0.35`) | `related` |
| `area` | Two or more shared leading `area` path components | `related` |
| `tags` | Two or more shared tags | `related` |

Clearing no rung yields `novel`. The reported `score` is the better of the trigram and word
measures whichever rung matched, which keeps the sort by score monotone in the ladder. The two
structural rungs need two shared components or tags rather than one, because one component is
the top-level crate directory that every row under a crate shares.

## Evidence directories

Each item may have a drop-box at `.claude/backlog-evidence/<item-id>/`. Three `.gitignore` rules
govern it — `/.claude/backlog-evidence/**` ignores everything beneath it,
`!/.claude/backlog-evidence/*/` re-includes the per-item directories, and
`!/.claude/backlog-evidence/*/.evidence` negates the marker back in — so the
directory survives into a fresh clone once its files have been left behind, and a screenshot
cannot be published by reflex. The directory re-include must precede the marker negation: git
will not re-include a file inside a directory that is itself still excluded, so the `**`
exclusion has to be lifted for the directory before the marker rule can take effect. Publishing
a file is a deliberate `git add -f <file>`, taken after reading it for credentials, personal
data and session tokens.

**Never hand-derive the path.** Ids widen on collision, so a directory built from an eyeballed
8-hex prefix is owned by nothing: `audit` reports it `unowned`, and it is invisible to `show`
and to `list --has-evidence`. Ask `evidence dir` for the path and copy into exactly what it
printed.

**The directory is the record.** Nothing in the store enumerates its files, and that is a
constraint rather than an omission: files arrive by a plain `cp` and leave by `rm`, with no
verb in between, so a stored count is wrong the moment one lands, a stored boolean wrong the
moment the last one is deleted, and a stored path a second copy of a name the filesystem
already owns. `show` and `list --has-evidence` read the directory live instead, which is also
what keeps "no directory" (`null`) distinguishable from "directory present, contents not in
this clone" (`files: []`).

Name a file for what it shows — a manual `cp` carries no caption, so the filename is the only
one it will ever have. Where a filename clarifies a sentence, reference it by that bare name in
the item's `context` prose: `audit` follows those references and reports `referenced-missing`
when the file is gone. A token is read as a filename only when it has a 2-to-8-character
extension containing a letter and no path separator, so `spawn.rs:214` reads as a source
pointer and `e.g` as prose rather than as missing files.

## Duration grammar

`--older-than` takes `<n>{s|m|h|d|w}`, the same grammar `flow stale --threshold` uses. A bare
number and an unknown suffix are both rejected with `kind=validation`.

## Frozen contracts

**The normaliser is frozen.** It folds a summary by lowercasing ASCII bytes, replacing every
non-alphanumeric byte with a space, and dropping a fixed stopword list; negations, modals
and verbs are deliberately kept, so "flakes" and "does not flake" cannot fold together. Every
`dedup_id` on disk is keyed on exactly that pipeline. Changing the byte folding or editing the
stopword list re-partitions the whole store: rows that used to collide stop colliding, ids
shift under the widening order, and `check` stops finding what it used to find. Treat any such
change as a `schema_version` bump requiring a rehash pass over both arrays — and note that no
such pass exists yet.

**A normaliser change cannot orphan an evidence directory**, because directory names derive
from the item id, which `evidence dir` resolves against the store rather than recomputing from
text. A rehash that moved ids would still have to rename the directories, but nothing silently
detaches in the meantime.
