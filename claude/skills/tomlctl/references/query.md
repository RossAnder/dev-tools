# tomlctl — query reference

The read-only half of the tomlctl surface: `get` / `parse` / `validate`, and the full
`items list` query surface — filters, projection, shaping, aggregation, output shapes —
plus `items get`, `items find-duplicates`, `items orphans`, and the tree-walking verbs
`sweep`, `items sweep` and `items clusters`. Nothing here writes to disk or touches a
sidecar; the mutating verbs live in [write.md](write.md), including the one opt-in write
on this page, `items sweep --update`.

## Contents

- [Read operations](#read-operations)
  - [Strict reads (`--strict-read`)](#strict-reads---strict-read)
- [Query `items` (full query surface)](#query-items-full-query-surface)
  - [Filters (all repeatable, all AND-combined)](#filters-all-repeatable-all-and-combined)
  - [Projection (mutually exclusive within this group)](#projection-mutually-exclusive-within-this-group)
  - [Shaping](#shaping)
  - [Aggregation (short-circuits projection / group-by)](#aggregation-short-circuits-projection--group-by)
  - [Output shapes (`--raw` / `--lines` / `--ndjson`)](#output-shapes---raw----lines----ndjson)
  - [Single-item fetch](#single-item-fetch)
  - [Find duplicates (read-only)](#find-duplicates-read-only)
  - [Surface orphans (read-only)](#surface-orphans-read-only)
- [Sweep and cluster](#sweep-and-cluster)
  - [Sweep the tree for patterns](#sweep-the-tree-for-patterns)
  - [Re-sweep a pattern item](#re-sweep-a-pattern-item)
  - [Cluster items for apply](#cluster-items-for-apply)

## Read operations

All read commands print JSON on stdout by default.

> **There is no `--format` / `--output` flag, by decision.** JSON is the only structured output; use `--raw` / `--lines` for bare scalars (see [Output shapes](#output-shapes---raw----lines----ndjson)). The compact table an agent reaches for is `--select <fields> --ndjson`: one self-describing object per line, immune to a `|` or tab inside a summary — which is exactly what a `--tsv` or a hand-rendered pipe table is not. `gh`/`kubectl`-style `--template` / `custom-columns` serve humans and shell scripts; tomlctl's reader is an agent, and NDJSON is strictly better for it. Do **not** invent `--format json`; it errors with `error: unexpected argument '--format' found` and a `Usage:` line for the subcommand, which names the flag to drop. Swallowing that (`2>/dev/null`) throws the diagnosis away and leaves the verification silently producing nothing.

```bash
# Whole document (omit path to read the entire file) or a single value
tomlctl get .claude/flows/auth-overhaul/context.toml
tomlctl get .claude/flows/auth-overhaul/context.toml status
tomlctl get .claude/flows/auth-overhaul/context.toml tasks.completed

# Scalar as bare text (no JSON quotes / no braces) — pipes straight into bash
tomlctl get .claude/flows/auth-overhaul/context.toml status --raw          # → review
tomlctl get .claude/flows/auth-overhaul/context.toml tasks.completed --raw # → 4

# Parse-check (exit 0 on valid)
tomlctl validate .claude/flows/auth-overhaul/context.toml
```

`--raw` on `get` requires a scalar target. It errors `--raw requires a scalar target; got {toml_type}` on a table or array.

TOML dates render as ISO-8601 strings in the JSON output (and as the ISO string in `--raw`).

`tomlctl parse <file>` remains accepted as a deprecated alias for `tomlctl get <file>` (no path argument) — kept for backward compatibility with older scripts. Prefer `tomlctl get <file>` in new docs and recipes.

### Strict reads (`--strict-read`)

By default the only read subcommand with a "missing file → silent default" branch is `items next-id --prefix <P>`, which returns `"<P>1"` as a bootstrapping fast path for flows that mint the first id before the ledger file exists. Every other read subcommand already errors on a missing file with `kind=not_found`.

Pass `--strict-read` when an agent needs to distinguish "no matches in an existing ledger" from "ledger does not exist" — e.g. when a flow expects a file to have been bootstrapped by `/plan-new` or `/implement` before proceeding:

```bash
# Errors with kind=not_found if the ledger hasn't been bootstrapped yet,
# even for next-id (which otherwise silently returns "R1").
tomlctl items next-id .claude/flows/foo/review-ledger.toml --prefix R --strict-read
tomlctl items list .claude/flows/foo/review-ledger.toml --status open --strict-read
```

`--strict-read` fires **before** `--verify-integrity`: a missing file under both flags yields `kind=not_found`, not `kind=integrity`. Zero-byte files are treated as a minimal valid doc in both modes; malformed TOML errors `kind=parse` in both modes.

## Query `items` (full query surface)

`tomlctl items list <file>` is the one-stop query tool for `[[items]]` (and any other array-of-tables via `--array <name>`). Every flag below is additive; omit any flag and it contributes nothing. Filters AND-combine; projections, shaping, and aggregation apply after filtering.

### Filters (all repeatable, all AND-combined)

| Operator | Usage | Meaning |
|---|---|---|
| `--where` | `--where status=open` | field equals value (exact match) |
| `--where-not` | `--where-not status=fixed` | field does not equal value |
| `--where-in` | `--where-in status=open,deferred,wontfix` | field in comma-separated set |
| `--where-has` | `--where-has defer_reason` | field present and non-empty |
| `--where-missing` | `--where-missing resolution` | field absent or empty |
| `--where-gt` / `--where-gte` | `--where-gte first_flagged=@date:2026-04-01` | field `>` / `>=` value |
| `--where-lt` / `--where-lte` | `--where-lt line=@int:100` | field `<` / `<=` value |
| `--where-contains` | `--where-contains summary=allocation` | field string contains substring |
| `--where-prefix` | `--where-prefix id=R2` | field string starts with |
| `--where-suffix` | `--where-suffix file=.rs` | field string ends with |
| `--where-regex` | `--where-regex symbol='^old::'` | caller-supplied regex (does NOT auto-anchor) |

**Typed RHS.** All `KEY=VAL` right-hand sides accept an optional `@type:` prefix to disambiguate native TOML types from string literals: `@date:`, `@datetime:`, `@int:`, `@float:`, `@bool:`, `@string:` / `@str:`. With no prefix the RHS is string, coerced to the field's native type when the field is typed.

**Legacy shortcut flags** (preserved; prefer `--where` for anything new): `--status <n>` ≡ `--where status=<n>`, `--category <n>` ≡ `--where category=<n>`, `--file <p>` ≡ `--where file=<p>`, `--newer-than <d>` ≡ `--where-gt first_flagged=@date:<d>`.

### Projection (mutually exclusive within this group)

```bash
# Keep only these keys per item
tomlctl items list ledger.toml --status open --select id,file,summary

# Drop these keys per item
tomlctl items list ledger.toml --status open --exclude description,evidence

# Flat list of one field's values
tomlctl items list ledger.toml --where-has defer_reason --pluck id
# → ["R3","R7","R22"]
```

`--select` + `--exclude`, `--select` + `--pluck`, and `--exclude` + `--pluck` are rejected at parse time.

### Shaping

```bash
# Sort ascending (default) or descending, tiebreakers via repeated flag
tomlctl items list ledger.toml --sort-by first_flagged
tomlctl items list ledger.toml --sort-by severity:desc --sort-by first_flagged:asc

# Paginate
tomlctl items list ledger.toml --limit 10
tomlctl items list ledger.toml --offset 20 --limit 10

# Dedup on the projected shape (preserve first occurrence)
tomlctl items list ledger.toml --select category --distinct
```

### Aggregation (short-circuits projection / group-by)

```bash
# Count matching items
tomlctl items list ledger.toml --status open --count
# → {"count": 7}

# Count distinct values of a field across matching items (replaces the
# --pluck F | jq -r '.[]' | sort -u | wc -l chain entirely).
tomlctl items list record.toml --where type=task-completion --count-distinct task_ref
# → {"count_distinct": 14, "field": "task_ref"}

# Bucket by a field, emit counts
tomlctl items list ledger.toml --count-by status
# → {"open": 7, "fixed": 12, "wontfix": 1}

# Bucket by a field, emit item lists
tomlctl items list ledger.toml --group-by file
# → {"src/a.rs": [item, ...], "src/b.rs": [item, ...]}
```

`--count`, `--count-distinct`, `--count-by`, `--group-by`, and `--pluck` are all members of the shape ArgGroup and are mutually exclusive.

### Output shapes (`--raw` / `--lines` / `--ndjson`)

- **`--raw`** — emit a single bare scalar (no JSON framing). Requires a shape that collapses to one value: `--count --raw`, `--count-distinct F --raw`, `--pluck F --raw` when exactly one item matches. Errors on multi-element pluck, `--count-by`, `--group-by`, or unfiltered list.
- **`--lines`** — emit one JSON value per line instead of a JSON array. Available only on `--pluck`.
- **`--ndjson`** — one item per line instead of a JSON array. Composes with `--select` / `--exclude`, so a projected list is one compact object per line — the agent-facing table shape. Unprojected, each line is a full item and pipes cleanly into `items add-many` / `items apply`.

```bash
tomlctl items list ledger.toml --status open --count --raw         # → 7
tomlctl items list ledger.toml --where id=R22 --pluck symbol --raw # → old::fn
tomlctl items list ledger.toml --status open --pluck id --lines    # R1\nR3\nR7
tomlctl items list ledger.toml --status open --ndjson              # {...}\n{...}
tomlctl items list ledger.toml --status open --select id,severity,summary --ndjson
# {"id":"R1","severity":"major","summary":"…"}
# {"id":"R3","severity":"minor","summary":"…"}
```

Anti-patterns, each seen in a real run — the output is already the shape you want, so never
post-process it:

- **Never merge stderr into a JSON parser's input.** `tomlctl … 2>&1 | python -c 'json.load(…)'` turns one tomlctl warning into a parser traceback that hides tomlctl's actual message. Leave stderr alone (and never `2>/dev/null` it either — see the note above).
- **Never re-render rows through a script.** `--select … --ndjson` already emits the projection; a hand-built pipe- or tab-delimited table breaks on the first summary containing that delimiter.
- **Never `| head -N` a list.** tomlctl has serialised everything by then; `--limit N` (with `--sort-by`) filters inside.
- **`items list` and `backlog list` return a bare array, not an envelope.** There is no `items` / `rows` / `backlog` key to guess at.

### Single-item fetch

```bash
tomlctl items get .claude/flows/auth-overhaul/review-ledger.toml R22
```

### Find duplicates (read-only)

`tomlctl items find-duplicates <ledger> [--tier A|B|C] [--across <other>]` surfaces likely-duplicate items without touching the ledger. Output is a JSON array of `{tier, key, items}` groups (empty array when no duplicates).

```bash
# Tier A (default): canonical dedup rule — group by (file, symbol) when
# symbol is non-empty, otherwise by (file, summary).
tomlctl items find-duplicates ledger.toml

# Tier B: content fingerprint. Groups items sharing
# <file>|<summary>|<severity>|<category>|<symbol> (truncated SHA-256, 16 hex)
# and the same file basename.
tomlctl items find-duplicates ledger.toml --tier B

# Tier C: file-scoped greedy line-window grouping for symbol-less items
# (group anchor + window of 10 lines).
tomlctl items find-duplicates ledger.toml --tier C
```

Cross-ledger with `--across`: runs tier A or B over the union of two ledgers. Each output entry is tagged with `source_file` (the basename of its origin ledger); the tag is applied at JSON-emit time and never written back to either on-disk ledger.

```bash
tomlctl items find-duplicates review-ledger.toml --across optimise-findings.toml --tier B
# [{"tier":"B","key":"…","items":[
#    {…,"source_file":"review-ledger.toml"},
#    {…,"source_file":"optimise-findings.toml"}]}, …]
```

Tier C is file-scoped by design (its line-window grouping assumes one source file) and errors under `--across`:

```
tier C is file-scoped; use --tier A or --tier B with --across
```

### Surface orphans (read-only)

`tomlctl items orphans <ledger>` walks every item and emits a JSON array of orphan records, one per detected class:

- `missing-file` — the item's `file` path does not exist under the repo root.
- `symbol-missing` — `file` exists but `symbol` is no longer a substring of its contents.
- `io-error` — `file` exists but cannot be read.
- `outside-repo` — `file` (absolute, or relative through `..`) escapes the repo root. Checked before the path is touched, so nothing outside the root is read.
- `dangling-dep` — one or more `depends_on = [...]` ids are not present in the ledger; the record lists them under `dangling_deps`.
- `instance-missing` — one `instances` anchor (`file:line` or `file:symbol`) does not resolve. The record carries the anchor verbatim under `instance` and a `reason`: `missing-file`, `symbol-missing`, `io-error` or `outside-repo`, meaning the same as the classes above but applied to the anchor's own file, or `unparseable` when the entry has no `:` tail after its last path separator. One record per failing anchor, in array order; a line anchor is checked for file existence only.

The file/symbol checks are mutually exclusive per path — the first failing one wins, in the order `outside-repo`, `missing-file`, `io-error`, `symbol-missing`.

```bash
tomlctl items orphans ledger.toml
# [{"id":"R7","class":"symbol-missing","file":"src/svc/foo.rs","symbol":"old::fn"},
#  {"id":"R7","class":"instance-missing","instance":"src/svc/bar.rs:old::fn","reason":"symbol-missing"},
#  {"id":"R9","class":"dangling-dep","dangling_deps":["R4"]}, ...]
```

An item can surface more than once: once on the file/symbol axis, once for its dangling deps, and once per unresolvable instance.

## Sweep and cluster

Three verbs that walk the working tree rather than the ledger alone. They replace the hand
procedures the research vet pass and the apply flow used to perform — re-running a finding's
search strings, and grouping selected items into file-disjoint clusters — so every site count
comes from one enumerator. All three need an installed binary of 0.9.0 or later
(`tomlctl capabilities` lists `sweep`, `items_sweep`, `items_clusters`, `orphans_instances`).

### Sweep the tree for patterns

`tomlctl sweep -e <regex>…` runs one or more regexes over the repo's tracked files and emits
sorted `file:line` sites. A research agent writes a pattern finding's `instances` line from
this output instead of transcribing Grep results. Files come from
`git ls-files --full-name -z --cached --others --exclude-standard` at the git top-level
(`TOMLCTL_ROOT` overrides the root; outside a git tree the verb errors with `kind=other`),
so untracked-but-not-ignored files are swept and ignored files are not. Hits are keyed by
canonical path, so two listings of one file — this repo mirrors `claude/agents` and each
`claude/skills/<name>` under `.github/` through directory symlinks git holds as ordinary
blobs — report once, under the path the symlink resolves to (`claude/…`). A symlink that
resolves outside the root is never read; it counts under `skipped.unreadable`.

```bash
tomlctl sweep -e '(?-u:\bfn items_orphans\b)'
tomlctl sweep -e '(?-u:\bold_name\b)' -e 'OldName::' --exclude 'vendor/**' --max-hits 200
```

```json
{
  "hits": [{"file": "tomlctl/src/orphans.rs", "line": 128, "pattern": 0}],
  "files_scanned": 662,
  "skipped": {"binary": 0, "oversize": 0, "unreadable": 0, "unenumerated": 1},
  "truncated": false,
  "coverage_complete": false
}
```

`hits` is sorted by file, then line; `pattern` is the index of the first `-e` that matched
the line, and a line matching several patterns or several times is one hit. `truncated` flips
to `true` at `--max-hits` distinct sites rather than erroring. `coverage_complete` is `true`
only when every `skipped` count is zero — a skipped file can hide a site, so a binary skip
counts. The `unenumerated: 1` above is real: a dangling directory symlink
(`.github/skills/flow-contract-task-visibility` today) makes `git ls-files` warn and continue
at exit 0, and that warning is surfaced as one unenumerated entry and clears
`coverage_complete`.

What the `git ls-files` basis does and does not cover:

- **Default exclusions**: `.claude/**` and `docs/plans/**` are dropped before scanning,
  because ledgers, the backlog and plan documents quote `sweep` strings and `instances`
  verbatim and every pattern would match its own record. `--exclude <GLOB>` adds to that
  list; it cannot remove from it.
- **Index entries, not disk files**: a staged-but-deleted file and a submodule gitlink both
  appear in the listing and fail to read; each counts under `skipped.unreadable` and never
  aborts the sweep. Submodule *contents* are never swept.
- **Very deep paths**: `core.longpaths` is unset on Windows, so a path over the platform
  limit can be dropped on git's side without a warning — a gap the tool cannot see.
- **Binary and oversize files**: a NUL byte in the first 8 KiB marks a file binary
  (ripgrep's heuristic, which also catches UTF-16 text); anything over `--max-file-bytes`
  skips. Both are counted, not fatal.

Patterns compile without Unicode tables — the installed binary is built with `regex`'s
`std` + `perf` features only — so `\b`, `\w`, `\d`, `\s` and case-insensitive `(?i)` on
non-ASCII fail to compile. Write the ASCII forms `(?-u:\b)`, `(?-u:\w)`, `(?-u:\d)`; the
compile error names that rewrite. Matching is multi-line with CRLF awareness (`$` does not
match before a `\r`), and a pattern longer than 512 bytes is refused — the same cap
`items list --where-regex` applies.

#### `sweep`

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--pattern` / `-e` | regex, repeatable | Pattern to search for. Required at least once. | — |
| `--max-file-bytes` | bytes | Files larger than this are skipped and counted under `skipped.oversize`. | `4194304` |
| `--max-hits` | count | Stop after this many distinct `file:line` sites and set `truncated: true`. | `5000` |
| `--exclude` | glob, repeatable | Extra exclusion on top of `.claude/**` and `docs/plans/**`. | none |

The verb takes no ledger and no integrity flag; `--error-format text|json` applies as
everywhere.

### Re-sweep a pattern item

`tomlctl items sweep <ledger>` binds the engine to a ledger: every selected item's stored
`sweep` strings run in one pass over the tracked files, each hit is attributed back to its
item, and the hits are diffed against the item's `instances` at anchor granularity. This is
what the vet pass and the apply pre-analysis call to answer "how many sites now, versus how
many the finding recorded" without a hand re-search. The ledger itself is excluded from its
own sweep on top of the default exclusions.

```bash
tomlctl items sweep <ledger> --ids R5,R12
tomlctl items sweep <ledger>
```

```json
{
  "items": [
    {
      "id": "R5",
      "recorded": ["src/a.rs", "src/b.rs"],
      "found": ["src/a.rs", "src/b.rs", "src/c.rs"],
      "new": ["src/b.rs:88", "src/c.rs:12"],
      "gone": [{"anchor": "src/a.rs:old_fn", "reason": "symbol-missing"}],
      "kept": ["src/a.rs:40", "src/b.rs:17"],
      "unverified": [],
      "coverage_complete": true,
      "truncated": false
    }
  ],
  "skipped_items": [{"id": "R7", "reason": "no-sweep"}],
  "files_scanned": 662,
  "coverage_complete": true,
  "truncated": false
}
```

Per item, `recorded` and `found` are file sets — the apply flow's file budget reads them —
and the four anchor lists partition every recorded anchor plus every hit:

- `new` — a `file:line` hit that no recorded `file:line` anchor names and that lies on no
  recorded `file:symbol` anchor's line, so growth inside an already-recorded file is visible.
- `gone` — a recorded anchor whose file was searched and has no hit at it (`reason: no-hit`),
  or a `file:symbol` anchor whose file has hits but no longer contains the symbol
  (`reason: symbol-missing`).
- `kept` — a recorded anchor the sweep confirmed.
- `unverified` — a recorded anchor the sweep could not judge: its file was skipped, lies
  past a `--max-hits` cut, or the entry did not parse as an anchor. Absence of a hit is not
  evidence there.

The item's `file` + `symbol` pair counts as an implicit anchor, so `instances` that repeat it
are not doubled. `skipped_items` names selections that swept nothing, with `reason` `no-sweep`
(the item has no `sweep` array) or `unknown-id` (the `--ids` value matches no item).

Read-only by default. `--update` rewrites each swept item's `instances` as its `kept` anchors
followed by its `new` sites and prints `{"ok":true,"updated":["R5"],"created":false,"path":…}`;
a run that changes nothing prints `"updated": []` and writes neither file nor sidecar.
`enumeration` is never rewritten — completeness is the agent's judgement about forms a regex
cannot reach — and `--update` refuses while any item is `truncated` or has `unverified`
anchors. The write path, its `--dry-run` preview and the anchor-spelling consequence (new
sites land as `file:line`, never as symbols) are documented in [write.md](write.md).

#### `items sweep`

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--ids` | comma-separated ids | Items to sweep. Omit for every item. | all |
| `--update` | — | Rewrite each swept item's `instances` from the results. Without it nothing is written and a missing ledger is `kind=not_found`, never created. | off |
| `--dry-run` | — | Preview the `--update` rewrite as a `would_change` summary; no file or sidecar touch. Requires `--update`. | off |
| `--max-file-bytes` | bytes | Files larger than this are skipped, which leaves their anchors `unverified`. | `4194304` |
| `--max-hits` | count | Distinct `file:line` sites after which the sweep stops and every item reports `truncated: true`. | `5000` |

The verb carries the write bundle (`--allow-outside`, `--no-create`, `--no-write-integrity`,
`--strict-integrity`, `--verify-integrity`) because of `--update`; there is no `--strict-read`
and no `--exclude` — the exclusion list is the defaults plus the ledger.

### Cluster items for apply

`tomlctl items clusters <ledger> --ids …` groups the selected items into file-disjoint
clusters and orders the clusters into dependency batches — the apply flow's Step 3 and the
dependency-sort contract's Kahn pass, computed rather than transcribed. An item's file set is
its `file` plus the files of its `instances`, keyed by canonical path so a mirrored file
counts once. Items are layered over `depends_on` first, then unioned on shared files only
within one layer, so two dependent items that touch the same file stay in sequential batches
with a commit between them. `depends_on` targets outside the selection are dropped and
listed under `dropped_deps`; a cycle inside the selection is refused with `kind=validation`
naming the items on it, and an `--ids` value matching no item is `kind=not_found`.

```bash
tomlctl items clusters <ledger> --ids R5,R12,R14
tomlctl items clusters <ledger>
```

```json
{
  "clusters": [
    {
      "id": "c1",
      "item_ids": ["R78"],
      "files": ["tomlctl/src/cli/dispatch/tests/lint.rs"],
      "depends_on": [],
      "lite_file_scope": true
    }
  ],
  "batches": [["c1"]],
  "dropped_deps": []
}
```

Cluster ids are assigned in first-item order; `batches` lists cluster ids per dependency
level, so every cluster in one batch may be dispatched in parallel once the previous batch
has landed. `lite_file_scope` is `true` when the cluster has at most two files, or holds
exactly one item whose `enumeration` is `complete` (a missing key counts as not complete).
It answers the lite-eligibility gate's file-scope criterion only; whether a multi-site
cluster shares one edit shape stays the orchestrator's judgement.

#### `items clusters`

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--ids` | comma-separated ids | Items to cluster. Omit for every `open` item. | every `open` item |

Read-only: carries `--verify-integrity` and `--strict-read` and no write flag.
