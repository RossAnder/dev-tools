---
name: tomlctl
description: Read and write TOML files used by Claude Code flows — `.claude/flows/*/context.toml`, `review-ledger.toml`, `optimise-findings.toml`. The single blessed path for parsing, querying, and mutating these files. Works on Windows and Linux; outputs JSON for easy consumption.
---

# tomlctl

> This document is the authoritative tomlctl reference. The top-level `tomlctl/README.md` is a short human tour that intentionally defers here for anything beyond the quick-tour examples.

A small Rust CLI that reads and writes the TOML files used by the `/plan-new`, `/implement`, `/plan-update`, `/review`, `/optimise`, `/review-apply`, and `/optimise-apply` commands.

## When to use this skill

Use `tomlctl` whenever a flow command needs to:

- Resolve a flow's `scope`, `branch`, `status`, or `artifacts.*` from `context.toml`.
- Read, filter, project, group, sort, count, or distinct-count `[[items]]` in `review-ledger.toml` / `optimise-findings.toml`.
- Update a single scalar (`status`, `updated`, `tasks.completed`) in `context.toml`.
- Append one or many new `[[items]]` entries, with optional pre-append dedup by named fields.
- Patch fields on an existing item by `id`, or unset fields.
- Append a record to a non-`items` array-of-tables (e.g. `[[rollback_events]]`).
- Compute the next `R{n}` / `O{n}` id — with an explicit `--prefix`, or inferred from the ledger.
- Backfill `dedup_id` on legacy ledgers, or surface duplicates (within a ledger or across two).
- Feature-gate downstream templates against a specific tomlctl version.
- Preview destructive mutations with `--dry-run` before committing them.

Every flow-TOML mutation routes through `tomlctl` — no Python, no line-level `Edit`, no `jq` for TOML parsing. Shell-level post-processing of tomlctl's JSON output is no longer needed either — prefer in-tool primitives (`--raw` / `--lines` / `--count-distinct` / `--count`) over piping through `jq -r .count` / `jq -r '.[]'` / `| sort -u | wc -l`.

## Install

One-time, per machine:

```bash
# from the dev-tools repo root
cargo install --path tomlctl
```

That drops `tomlctl` into `~/.cargo/bin/` (already on PATH if Rust is installed). Verify:

```bash
tomlctl --version
```

## Feature-gate with `tomlctl capabilities`

`tomlctl capabilities` emits a stable JSON document (`{"version":"…","features":[…],"subcommands":[…]}`) so downstream templates can feature-gate at boot without parsing `--help` prose. Features are stable within a minor release; new flags add new feature entries rather than being version-qualified.

Features:

| Feature | What it enables |
|---|---|
| `count_distinct` | `--count-distinct <FIELD>` on `items list` |
| `raw` | `--raw` scalar emit on `get` and on single-value `items list` shapes |
| `lines` | `--lines` newline-per-value emit on `items list --pluck` |
| `infer_prefix` | `items next-id --infer-from-file` |
| `dedupe_by` | `--dedupe-by <FIELDS>` on `items add` / `items add-many` |
| `dedup_id_auto` | auto-populate `dedup_id` in every write funnel |
| `find_duplicates_across` | `items find-duplicates --across <other>` cross-ledger tier A/B |
| `capabilities` | this subcommand itself |
| `error_format_json` | `--error-format json` global flag + `ErrorKind` taxonomy |
| `strict_read` | `--strict-read` on every read subcommand |
| `dry_run` | `--dry-run` on `items remove` / `items apply` / `items backfill-dedup-id` |
| `backfill_dedup_id` | `items backfill-dedup-id <file>` |
| `integrity_refresh` | `integrity refresh <file>` — materialise / regenerate the `.sha256` sidecar against the file's current on-disk bytes |

## Read operations

All read commands print JSON on stdout by default.

```bash
# Whole document as JSON (omit the path argument to read the entire file)
tomlctl get .claude/flows/auth-overhaul/context.toml

# Single value at a dotted path
tomlctl get .claude/flows/auth-overhaul/context.toml status
tomlctl get .claude/flows/auth-overhaul/context.toml tasks.completed
tomlctl get .claude/flows/auth-overhaul/context.toml artifacts.optimise_findings

# Scalar as bare text (no JSON quotes / no braces) — pipes straight into bash
tomlctl get .claude/flows/auth-overhaul/context.toml status --raw
# → review
tomlctl get .claude/flows/auth-overhaul/context.toml tasks.completed --raw
# → 4

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
tomlctl --strict-read items next-id .claude/flows/foo/review-ledger.toml --prefix R
tomlctl --strict-read items list .claude/flows/foo/review-ledger.toml --status open
```

`--strict-read` fires **before** `--verify-integrity`: a missing file under both flags yields `kind=not_found`, not `kind=integrity`. Zero-byte files are treated as a minimal valid doc in both modes; malformed TOML errors `kind=parse` in both modes.

## Query `items` (full query surface)

`tomlctl items list <file>` is the one-stop query tool for `[[items]]` (and any other array-of-tables via `--array <name>`). Every flag below is additive; omit any flag and it contributes nothing. Filters AND-combine; projections, shaping, and aggregation apply after filtering.

### Filters (all repeatable, all AND-combined)

All `KEY=VAL` right-hand sides accept an optional `@type:` prefix to disambiguate native TOML types from string literals:

| RHS form | Meaning |
|---|---|
| `@date:2026-04-18` | TOML date literal |
| `@datetime:2026-04-18T10:00:00Z` | TOML datetime |
| `@int:42` | integer |
| `@float:1.5` | float |
| `@bool:true` | boolean |
| `@string:foo` / `@str:foo` | explicit string (useful when value looks like a date/int but you need string compare) |
| `foo` (no prefix) | string; when the item field is natively typed, the RHS is coerced to the field's type before comparison |

```bash
# Status/category/file — legacy shortcut flags still work (unchanged semantics)
tomlctl items list ledger.toml --status open
tomlctl items list ledger.toml --category quality --status open --file src/auth/session.rs

# Generic equality (exact match; string or native-typed)
tomlctl items list ledger.toml --where status=open
tomlctl items list ledger.toml --where severity=critical --where category=memory

# Negated equality
tomlctl items list ledger.toml --where-not status=fixed

# Set membership
tomlctl items list ledger.toml --where-in status=open,deferred,wontfix

# Field presence
tomlctl items list ledger.toml --where-has defer_reason      # field present and non-empty
tomlctl items list ledger.toml --where-missing resolution    # field absent or empty

# Numeric / date comparison (use @type: when RHS is ambiguous)
tomlctl items list ledger.toml --where-gte first_flagged=@date:2026-04-01
tomlctl items list ledger.toml --where-lt line=@int:100
tomlctl items list ledger.toml --where-gt rounds=@int:1

# String substring / prefix / suffix (case-sensitive)
tomlctl items list ledger.toml --where-contains summary=allocation
tomlctl items list ledger.toml --where-prefix id=R2
tomlctl items list ledger.toml --where-suffix file=.rs

# Regex (caller-supplied anchors — does NOT auto-anchor)
tomlctl items list ledger.toml --where-regex symbol='^old::'
```

Legacy shortcut flags preserved (use `--where` for anything new):

- `--status <name>` — same as `--where status=<name>`
- `--category <name>` — same as `--where category=<name>`
- `--file <path>` — same as `--where file=<path>`
- `--newer-than <YYYY-MM-DD>` — same as `--where-gt first_flagged=@date:<d>`

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

### Output shapes (`--raw` / `--lines`)

`--raw` emits a single scalar with no JSON framing (no quotes on strings, no object braces) — pipes straight into bash arithmetic or string comparison. It requires a shape that collapses to exactly one value:

```bash
# Bare integer, no {"count": N} wrapping
tomlctl items list ledger.toml --status open --count --raw
# → 7

# Bare integer, no {"count_distinct":...,"field":...} wrapping
tomlctl items list record.toml --where type=task-completion \
  --count-distinct task_ref --raw
# → 14

# Single pluck result as a bare string
tomlctl items list ledger.toml --where id=R22 --pluck symbol --raw
# → old::fn
```

Multi-element pluck with `--raw` errors: `--raw requires single-value output (got {N} items); use --lines for newline-delimited`. Non-single-value shapes (`--count-by`, `--group-by`, unfiltered list) also error under `--raw`.

`--lines` emits one JSON value per line (newline-delimited) instead of a JSON array — lets downstream shell iterate a pluck without `jq -r '.[]'`:

```bash
# Each id on its own line, no JSON array wrapper
tomlctl items list ledger.toml --status open --pluck id --lines
# R1
# R3
# R7
```

`--lines` is available only on `--pluck`. On other shapes it errors — use `--ndjson` for per-row streaming of full items.

### NDJSON output (row streaming)

```bash
# Newline-delimited one-item-per-line output — pipes cleanly into items add-many / apply
tomlctl items list ledger.toml --status open --ndjson
```

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

### Verify shared-block parity across markdown files

`tomlctl blocks verify` checks that a named shared block is byte-identical across a set of files, mirroring `scripts/verify-shared-blocks.sh` without the bash+awk dependency. Blocks are delimited by `<!-- SHARED-BLOCK:<name> START -->` … `<!-- SHARED-BLOCK:<name> END -->` markers (inclusive markers excluded from the hash).

```bash
# Verify named blocks across all four command files
tomlctl blocks verify claude/commands/optimise.md claude/commands/review.md \
  claude/commands/optimise-apply.md claude/commands/review-apply.md \
  --block flow-context --block ledger-schema

# Omit --block to verify every block present in the first listed file
tomlctl blocks verify claude/commands/*.md
```

Output is JSON (`{"ok":true|false,"blocks":[...]}`). Exit code is 0 on success, non-zero on drift or missing markers.

## Write operations

Writes preserve every field the tool didn't touch, including `created`. Key order within tables is preserved.

### Set a scalar at a path

```bash
# Type is auto-inferred: YYYY-MM-DD → date, true/false → bool, digits → int, else string
tomlctl set .claude/flows/auth-overhaul/context.toml status review
tomlctl set .claude/flows/auth-overhaul/context.toml updated 2026-04-17
tomlctl set .claude/flows/auth-overhaul/context.toml tasks.completed 4

# Force a specific type when inference would go wrong
tomlctl set path/to/file.toml some_key 42 --type str
tomlctl set path/to/file.toml when 2026-04-17T10:00:00Z --type datetime
```

Supported `--type` values: `str`, `int`, `float`, `bool`, `date`, `datetime`.

### Set an array or object at a path (`set-json`)

When the target isn't a scalar (e.g. `scope`, `[artifacts]` as a whole), pass a JSON-encoded value with `set-json`. ISO-date strings (`YYYY-MM-DD`) are auto-promoted to TOML date literals, same as `items add` / `items update`.

```bash
# Refresh scope array (e.g. during /plan-update reconcile)
tomlctl set-json .claude/flows/auth/context.toml scope \
  --json '["src/auth/**","src/routes/**","src/middleware/auth.rs"]'

# Replace a whole subtable
tomlctl set-json .claude/flows/auth/context.toml artifacts \
  --json '{"review_ledger":"x.toml","optimise_findings":"y.toml"}'
```

### Append a single new item

`--json` takes one JSON object representing the new `[[items]]` entry. Field order in the JSON becomes field order in the emitted TOML, so pass fields in the canonical key order from `## Ledger Schema`:
`id, file, line, symbol, severity, effort, category, summary, description, evidence, first_flagged, rounds, related, status, <disposition-specific>, flow`.

```bash
tomlctl items add .claude/flows/foo/optimise-findings.toml --json '{
  "id": "O7",
  "file": "src/svc/foo.rs",
  "line": 44,
  "severity": "critical",
  "effort": "small",
  "category": "memory",
  "summary": "Allocates fresh Vec in hot loop",
  "first_flagged": "2026-04-17",
  "rounds": 1,
  "status": "open"
}'
```

`dedup_id` is auto-populated by the write funnel if the payload doesn't set it — see [Dedup fingerprint contract](#dedup-fingerprint-contract). Rendered output (e.g. PROGRESS-LOG columns) is unaffected; the field only appears in the TOML.

Date-shaped strings (`YYYY-MM-DD`) in the `DATE_KEYS` set (`created`, `updated`, `first_flagged`, `last_updated`, `resolved`, `date`) are automatically promoted to TOML date literals.

#### Pre-append dedup (`--dedupe-by`)

`--dedupe-by <FIELDS>` on `items add` / `items add-many` rejects rows whose named fields exactly match an existing item. `FIELDS` is a comma-separated list; comparison is raw equality on each named field's string form. Does NOT implicitly include `dedup_id`; pass `--dedupe-by dedup_id` explicitly to use fingerprint-based dedup. The pre-scan runs BEFORE `dedup_id` auto-populate, so a payload's auto-populated `dedup_id` never influences its own pre-scan.

```bash
# Reject rows where (file, summary) matches any existing row
tomlctl items add ledger.toml --dedupe-by file,summary --json '{...}'

# Fingerprint-based dedup
tomlctl items add ledger.toml --dedupe-by dedup_id --json '{...}'
```

### Batch append many items — `items add-many`

For runs that need to append many new items at once (e.g. a 50-finding review batch), assemble NDJSON line-by-line and pass it to `items add-many`. Each line is one JSON object; blank lines are ignored; any malformed line aborts the whole batch pre-mutation and names the offending line number.

**Default to the staging-file form** — write the NDJSON to a sibling file and pass `--ndjson <path>`. It works identically on every platform, survives payloads of any size, and sidesteps the Windows Git Bash heredoc breakage described in [Stdin input for large JSON payloads](#stdin-input-for-large-json-payloads).

```bash
# 1. Stage the batch (Write tool or `cat > …` — payload doesn't touch the shell).
# 2. Invoke with --ndjson pointing at that file:
tomlctl items add-many .claude/flows/foo/review-ledger.toml \
  --defaults-json '{"first_flagged":"2026-04-18","rounds":1,"status":"open"}' \
  --ndjson .claude/flows/foo/_batch.ndjson
# → {"ok":true,"added":N}
# Delete _batch.ndjson after the call.
```

On Unix shells you can inline the payload with a heredoc (`--ndjson - <<'EOF' … EOF`). Do **not** reach for that form on Windows Git Bash — multi-line heredocs intermittently fail with `unexpected EOF while looking for matching \`''` because CRLF line endings break bash's terminator match under `bash -c`. The file form above is the safe default everywhere.

`--array <name>` targets a non-default array-of-tables. `--defaults-json` is optional; omit it for rows that are already fully-formed. `--dedupe-by <FIELDS>` as on `items add`.

Prefer `items add-many` over a shell loop of single `items add` calls — one parse, one lock, one rewrite, one sidecar refresh.

### Patch an existing item

Matched by `id`. The JSON object is merged into the item (shallow). Existing unmentioned fields stay untouched.

```bash
# Mark an item applied with resolution commit
tomlctl items update .claude/flows/foo/review-ledger.toml R22 --json '{
  "status": "applied",
  "resolved": "2026-04-17",
  "resolution": "Fixed in ab12cd3"
}'

# Increment rounds (read current, then set)
tomlctl items update .claude/flows/foo/review-ledger.toml R22 --json '{"rounds": 2}'
```

`dedup_id` is recomputed by the write funnel when the patch touches a fingerprinted field (`file`, `summary`, `severity`, `category`, `symbol`) and does not set `dedup_id` explicitly. See [Dedup fingerprint contract](#dedup-fingerprint-contract).

#### Unset fields

`--unset <key>` (repeatable) drops a field from the matched item. The patch is applied **first**, then each unset runs, so an `--unset` on the same key as a `--json` set wins. Unsetting a key that is not present is silently a no-op — field-absent is the desired end state.

`--json` is still required; pass `--json '{}'` when you only want to unset:

```bash
# Flip deferred -> open and drop the defer triggers in a single rewrite
tomlctl items update ledger.toml R7 \
  --json '{"status":"open","rounds":2}' \
  --unset defer_reason --unset defer_trigger
```

In `items apply` batches, add an optional `unset` array of strings to an `update` op. Back-compat: omitting `unset` leaves behaviour unchanged.

```bash
tomlctl items apply ledger.toml --ops '[
  {"op":"update","id":"R7","json":{"status":"open"},"unset":["defer_reason","defer_trigger"]}
]'
```

### Remove an item

Rare — IDs are never renumbered per spec — but occasionally needed for manual cleanup. Fails if the id does not exist.

```bash
tomlctl items remove .claude/flows/foo/review-ledger.toml R17

# Preview with --dry-run — reports the computed mutation without touching disk
tomlctl items remove .claude/flows/foo/review-ledger.toml R17 --dry-run
# → {"ok":true,"dry_run":true,"would_change":{"added":0,"updated":0,"removed":1,"ids":["R17"]}}
```

### Batch multiple mixed item ops (`items apply`)

For runs that mix add/update/remove on `[[items]]` in the same ledger, use `items apply` to parse + rewrite the file once. `--ops` is a JSON array; each op is `{"op": "add|update|remove", ...}` with the same payload shape as the single-op commands (`json` for add/update, `id` for update/remove). Ops run in array order; any op error aborts the whole batch and the file is left unchanged.

```bash
tomlctl items apply .claude/flows/foo/review-ledger.toml --ops '[
  {"op":"add",    "json":{"id":"R24","severity":"minor","summary":"...","status":"open"}},
  {"op":"update", "id":"R22", "json":{"status":"applied","resolved":"2026-04-17"}},
  {"op":"remove", "id":"R17"}
]'
```

Prefer this over looping single-op invocations — one parse + one write instead of N. For homogeneous add-only batches prefer `items add-many` (simpler input shape). For append-only non-`items` arrays prefer `array-append`.

Preview with `--dry-run`:

```bash
tomlctl items apply ledger.toml --ops '[...]' --dry-run
# → {"ok":true,"dry_run":true,"would_change":{"added":1,"updated":1,"removed":1,"ids":["R17","R22","R24"]}}
```

The dry-run path runs the same compute stage as the real path — mutation logic cannot drift between preview and apply.

#### Targeting a non-default array-of-tables (`--array`)

`items apply` defaults to mutating the `[[items]]` array at the ledger root. Pass `--array <name>` to redirect the batch at a different array-of-tables (e.g. `rollback_events`). `--array` is accepted on `items list`, `items get`, `items add`, `items add-many`, `items update`, `items remove`, and `items apply` — so any of these can target a non-default array such as `rollback_events`. `items next-id`, `items find-duplicates`, and `items orphans` do not take `--array` (they are ledger-schema specific and only reason about `[[items]]`).

### Compute the next id

```bash
# Explicit prefix (required unless --infer-from-file is passed)
tomlctl items next-id .claude/flows/foo/review-ledger.toml --prefix R     # → "R23"
tomlctl items next-id .claude/flows/foo/optimise-findings.toml --prefix O # → "O1" on empty

# Infer the prefix from existing items — the ledger must be non-empty AND
# contain exactly one prefix. Errors otherwise:
#   "--infer-from-file requires a non-empty ledger or explicit --prefix"
#   "--infer-from-file found multiple prefixes (R, O); pass --prefix explicitly"
tomlctl items next-id .claude/flows/foo/review-ledger.toml --infer-from-file
# → "R23"
```

`--prefix` and `--infer-from-file` are mutually exclusive (one is required). `--prefix` on a missing file returns `<prefix>1` as a bootstrapping fast path (see [Strict reads](#strict-reads---strict-read) for how to disable that default). `--infer-from-file` cannot bootstrap — it needs existing items to infer from.

Returns the JSON-encoded string of the next id (prefix + `max(existing numeric suffixes) + 1`).

### Append to an array-of-tables — `array-append`

For append-only arrays such as `[[rollback_events]]` (written by `/review-apply` / `/optimise-apply` rollback protocol), use `array-append`. It's a thin shim over `items add-many` that targets an arbitrary array name and doesn't require op-type framing.

```bash
# Single record
tomlctl array-append <ledger> rollback_events --json '{
  "timestamp": "2026-04-18T14:32:00Z",
  "command": "review-apply",
  "cause": "build failure",
  "items": ["R3","R7"],
  "stash_ref": "stash@{0}"
}'

# Many records via NDJSON — stage to a sibling file and pass --ndjson <path>.
# Same platform reasoning as `items add-many` above; avoid heredocs on Windows Git Bash.
tomlctl array-append <ledger> rollback_events \
  --ndjson .claude/flows/foo/_rollback-batch.ndjson
```

`items apply --array <name>` remains available for heterogeneous batches (add/update/remove on the same array in one parse+write). Use `array-append` when every op is an append.

### Migrate legacy ledgers — `items backfill-dedup-id`

Ledgers created before 0.2.0 have no `dedup_id` field on any item. `items backfill-dedup-id` computes and writes the fingerprint for every item that lacks one, preserving any item that already has a (possibly manually set) value. Idempotent — a second run is a no-op.

```bash
# Preview
tomlctl items backfill-dedup-id .claude/flows/foo/review-ledger.toml --dry-run
# → {"ok":true,"dry_run":true,"would_backfill":23,"ids":["R1","R2",...]}

# Apply (returns backfilled:0 when there's nothing to do — idempotent, write skipped)
tomlctl items backfill-dedup-id .claude/flows/foo/review-ledger.toml
# → {"ok":true,"backfilled":23}

# Kill switch engaged — short-circuits without reading the file
TOMLCTL_NO_DEDUP_ID=1 tomlctl items backfill-dedup-id <ledger>
# → {"ok":true,"backfilled":0,"reason":"disabled-by-env"}
```

### Regenerate a missing sidecar — `integrity refresh`

Materialises (or regenerates) the `<file>.sha256` sidecar from the file's current on-disk bytes. Does NOT modify the TOML — use this when the sidecar is absent or lost but the TOML is authoritative as-is.

```bash
# Bootstrap: /plan-new's Write of the 2-line execution-record.toml
# skeleton bypasses the tomlctl write pipeline, so no sidecar is produced.
# Run integrity refresh immediately after the Write to close the gap.
tomlctl integrity refresh .claude/flows/<slug>/execution-record.toml
# → {"ok":true}

# Recovery: sidecar deleted out-of-band (git clean, stray rm), TOML intact.
tomlctl integrity refresh .claude/flows/<slug>/review-ledger.toml
```

Acquires the same exclusive lock a write path would, so it serialises correctly with concurrent writers. Subject to the same `.claude/` containment guard as other write paths — pass `--allow-outside` to refresh a sidecar for a file outside `.claude/`. Calling this on a file that already has a valid sidecar is a no-op-ish (it rewrites the sidecar with the same bytes) and idempotent.

### Stdin input for large JSON payloads

All JSON-accepting flags (`--ops`, `--json` on `items add` / `items update` / `set-json`, `--defaults-json` / `--ndjson` on `items add-many` / `array-append`) treat the literal value `-` as "read from stdin". Use this to avoid shell-quoting or tempfile round-trips when the payload is large or contains quotes / newlines / dollar signs.

Stdin consumption rules:

- Refuses to block on an interactive TTY (so `… --json -` without a pipe errors fast rather than hanging).
- Caps the read at 32 MiB.
- Only one flag per invocation may use `-` — a second `-` on the same call errors with `stdin already consumed by another flag on this invocation`.

Prefer heredocs over tempfiles for on-invocation staging:

```bash
tomlctl items add-many ledger.toml --ndjson - <<'EOF'
{"id":"R1", ...}
{"id":"R2", ...}
EOF
```

**Windows Git Bash fallback.** If a heredoc errors with `unexpected EOF while looking for matching \`''` (CRLF line endings break the `EOF` terminator match inside `bash -c`), write the payload to a sibling file and avoid the heredoc:

- `--ndjson` also accepts a file path directly: `--ndjson <path>`.
- For `--json` / `--ops` / `--defaults-json` (which accept only a literal or `-`), pipe the file in: `cat <path> | tomlctl … --json -`.

Both forms work identically on every platform; delete the staging file after the call.

## Dedup fingerprint contract

Every write funnel (`items add`, `items add-many`, `items update`, `items apply`) auto-populates a `dedup_id` field per these rules:

- **add / add-many**: if the payload lacks `dedup_id`, it's computed from the payload.
- **update / apply**: branch order below — first match wins:
  1. Patch explicitly sets `dedup_id` (non-empty string) → preserve caller's value.
  2. Patch touches a fingerprinted field AND does not set `dedup_id` → recompute from the merged (patch-over-existing) view.
  3. Patch touches no fingerprinted field AND existing item lacks `dedup_id` → leave absent. Unrelated updates on legacy ledgers do NOT silently populate; use `items backfill-dedup-id` to upgrade.
  4. Patch touches no fingerprinted field AND existing item has `dedup_id` → preserve.

`items update --json '{"dedup_id":null}'` is treated as "patch didn't mention the field" (branch 3 or 4, depending on existing state) — the less-surprising semantics. Use `--unset dedup_id` or an explicit non-empty value to force a change.

**Fingerprint formula.** `sha256(file|summary|severity|category|symbol)` — each field read as a string (empty string for missing / non-string values); no trimming or normalisation; field order is load-bearing and matches `tomlctl items find-duplicates --tier B`. The digest is truncated to 16 hex chars (64 bits). Birthday-bound at ~4B items per scope; set `dedup_id` explicitly on the payload for adversarial inputs.

**Rollback lever.** `TOMLCTL_NO_DEDUP_ID=1` disables auto-populate globally. Any value (even empty) disables the hook; unset the env var to re-enable. With the kill switch engaged, `items backfill-dedup-id` short-circuits with `{"ok":true,"backfilled":0,"reason":"disabled-by-env"}`.

**`--dedupe-by` interaction.** `--dedupe-by <FIELDS>` on `items add` / `items add-many` does NOT implicitly include `dedup_id`. Callers wanting fingerprint-based dedup pass `--dedupe-by dedup_id` explicitly. The dedupe pre-scan always runs BEFORE auto-populate, so a payload's auto-populated `dedup_id` never influences its own pre-scan.

## Error format (`--error-format json`)

`--error-format json` is a global flag on the top-level command. When set, errors are written to **stderr** as a compact single-line JSON envelope:

```
{"error":{"kind":"<kind>","message":"<prose>","file":null|"<path>"}}
```

Exit code stays 1 on error. Success paths are unchanged — text output on success is byte-identical to default mode.

```bash
tomlctl --error-format json items list /nonexistent/ledger.toml 2>&1 >/dev/null
# {"error":{"kind":"not_found","message":"...","file":"/nonexistent/ledger.toml"}}
```

Closed taxonomy (every tag site is enumerated; all other `bail!` sites fall through to `other`):

| `kind` | Emitted from |
|---|---|
| `not_found` | `io.rs` — target file missing at the path the caller passed |
| `integrity` | `integrity.rs` — sidecar hash mismatch or missing under `--verify-integrity` |
| `parse` | `io.rs` — malformed TOML at the document root |
| `validation` | `query.rs` / `items.rs` — flag-mutex violations, `items next-id` prefix shape rejections, `--infer-from-file` empty/multi-prefix errors |
| `other` | any untagged error — the downcast returned `None` |

Prefer `--error-format json` + `.error.kind` switching over regex-matching stderr text when branching on error class (e.g. "bootstrap the ledger if missing, bubble up otherwise").

## Integrity sidecar

Every write emits an integrity sidecar next to the target: `<file>.sha256`, in the standard `sha256sum` format (`<64-lower-hex>  <basename>\n`, two spaces between the digest and the basename, trailing newline). The sidecar is written atomically via tempfile + rename under the same lock as the primary write, so an interleaved `--verify-integrity` reader never sees a torn pair.

**Threat model.** The sidecar is a consistency check against accidental corruption and collaborative out-of-band edits — it is **not** a MAC or tamper-proof signature. An attacker with ledger write access can trivially rewrite the sidecar; hostile-actor threat models still require auditing the ledger's git history.

```bash
# Default behaviour: write writes both ledger.toml and ledger.toml.sha256
tomlctl items update ledger.toml R7 --json '{"status":"fixed"}'

# Skip the sidecar for this invocation (e.g. read-only-ish filesystems or
# when you want to hand-edit before the next write regenerates it).
tomlctl --no-write-integrity items update ledger.toml R7 --json '{"status":"fixed"}'

# Treat sidecar write failures as hard errors instead of warnings.
tomlctl --strict-integrity items update ledger.toml R7 --json '{"status":"fixed"}'
```

Pass `--verify-integrity` on any invocation to verify the target against its sidecar before every read. Wires into `parse`, `get`, `validate`, `items list`, `items get`, `items next-id`, `items find-duplicates`, `items orphans`.

```bash
tomlctl --verify-integrity items list ledger.toml --status open
# If the sidecar is missing OR the digest disagrees, the command exits
# non-zero and the error names both hashes + the sidecar path.
```

- **Missing sidecar with `--verify-integrity`** → error names the expected sidecar path; never auto-regenerated.
- **Digest mismatch** → error names both the expected (from sidecar) and actual (from current file bytes) digests. Resolve by a human (either the file drifted out-of-band or the sidecar is stale). `tomlctl` will never auto-repair.
- **Sidecar write failure** (disk full, permissions, etc.) after the primary write has landed → by default, logged to stderr as a warning; the command still exits 0 (data is durable; sidecar can be rebuilt by any subsequent write). `--strict-integrity` flips this to a hard error.
- **Same write-guard applies** — sidecars are written alongside the target, so a write that passed `--allow-outside` writes its sidecar to the same location.

## Constraints and gotchas

- **No comment preservation.** The schemas forbid inline comments, so this is fine for flow/ledger files. Do not point `tomlctl` at TOML files where comments matter.
- **Whole-file rewrite.** Any write operation reparses, mutates, and re-serialises the whole document. Never runs a line-level Edit.
- **Whitespace may change.** Long inline arrays may be reflowed to multi-line by the serializer. Semantically identical.
- **`created` is preserved verbatim.** The tool never touches it unless you explicitly `set created <date>` (don't).
- **`dedup_id` auto-populates on every write** unless `TOMLCTL_NO_DEDUP_ID=1`. First-time upgrade of a legacy item (add/add-many path) populates without marking it as a user-intended change — the sidecar refresh is an implicit one-time event.
- **Unknown-value rules stay with the caller.** `tomlctl` returns raw values; the command's "unknown status → treat as in-progress" / "unknown category → fail-soft" rules apply in the calling command's logic, not in the tool.
- **Errors exit non-zero and print to stderr.** Success paths emit either JSON data (or `--raw` / `--lines` bare text) or `{"ok":true,…}` to stdout. Always check exit code in scripted flows. For machine-readable error class, use `--error-format json`.
- **Lock timeout: 30 seconds.** Writes acquire an exclusive OS-level lock on a hashed lock file under `<repo-top-level>/.claude/.locks/<sha256-of-canonical-target-path>.lock` (O44 moved the lock location off the sidecar `<file>.toml.lock` scheme to avoid collision with real files named `foo.toml.lock`). `tomlctl` polls `try_lock_exclusive` on this file and bails after 30 s total with an error naming the lock path. On Windows this is a mandatory lock — a crashed or stuck `tomlctl` leaves the `.lock` file present and the OS keeps the lock until the offending process dies. **Recovery when a lock is stranded:** confirm no live `tomlctl` process holds it (Task Manager / `Get-Process tomlctl` / `ps aux | grep tomlctl`), then delete the specific `.claude/.locks/<hash>.lock` file from the error message. The next invocation will recreate and re-acquire it cleanly.
- **Write-path safety (best-effort containment guard, not a sandbox).** Write operations (`set`, `set-json`, `items add|update|remove|apply|add-many|backfill-dedup-id`, `array-append`) reject targets that canonicalise outside the current repo's `.claude/` directory. The guard resolves symlinks and `..` at canonicalisation time and rejects paths not under `<git-top-level>/.claude/`. Read operations are not guarded. Pass `--allow-outside` (a per-subcommand flag) to override when you genuinely need to edit a flow TOML elsewhere — e.g. `tomlctl set /tmp/scratch.toml status draft --allow-outside`. `--allow-outside` is pinned behind an interactive permission prompt at the project settings level — it should never appear in unattended automation. Treat this as a best-effort guard against agent/user typos that would otherwise land writes in unintended locations; it is not a security sandbox and a TOCTOU-race or symlink swap between canonicalisation and open can in principle escape it.

## Permissions

`Bash(tomlctl *)` is pre-approved in the project's `.claude/settings.json`. `Bash(tomlctl --allow-outside *)` is explicitly denied at the same layer, so any invocation passing `--allow-outside` falls through to an interactive permission prompt. Agents should never emit `--allow-outside` unattended — the write-path containment guard is default-on for a reason.
