# tomlctl — query reference

The read-only half of the tomlctl surface: `get` / `parse` / `validate`, and the full
`items list` query surface — filters, projection, shaping, aggregation, output shapes —
plus `items get`, `items find-duplicates`, and `items orphans`. Nothing here writes to
disk or touches a sidecar; the mutating verbs live in [write.md](write.md).

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
- `dangling-dep` — one or more `depends_on = [...]` ids are not present in the ledger.

```bash
tomlctl items orphans ledger.toml
# [{"id":"R7","class":"symbol-missing","file":"src/svc/foo.rs","symbol":"old::fn"}, ...]
```

An item can surface twice if it is both file/symbol-orphaned AND has dangling deps.
