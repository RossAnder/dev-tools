---
name: tomlctl
description: Read and write TOML files used by Claude Code flows — `.claude/flows/*/context.toml`, `review-ledger.toml`, `optimise-findings.toml`. The single blessed path for parsing, querying, and mutating these files. Works on Windows and Linux; outputs JSON for easy consumption.
---

# tomlctl

> This document is the authoritative tomlctl reference. The top-level `tomlctl/README.md` is a short human tour that intentionally defers here for anything beyond the quick-tour examples.

A small Rust CLI that reads and writes the TOML files used by the `/plan-new`, `/implement`, `/plan-update`, `/review`, `/optimise`, `/review-apply`, and `/optimise-apply` commands.

## Quick Reference

The highest-frequency patterns. Deeper treatment in the linked sections.

| Task | Command |
|---|---|
| Append one item (JSON arg) | `tomlctl items add <file> --json '{...}'` |
| Append one item (stdin) | `cat payload.json \| tomlctl items add <file> --json -` |
| Batch append homogeneous items | `tomlctl items add-many <file> --ndjson <path>` |
| Apply heterogeneous batch (add/update/remove) | `tomlctl items apply <file> --ops -` |
| Filter items | `tomlctl items list <file> --where status=open` ([see filter operators](#filters-all-repeatable-all-and-combined)) |
| Count / bucket items | `tomlctl items list <file> --count` / `--count-by status` / `--group-by file` |
| Next monotonic id | `tomlctl items next-id <file> --prefix R\|O\|E\|P` |
| Bump scalar field | `tomlctl set <file> <key.path> <value>` |
| Set array / sub-table | `tomlctl set-json <file> <key.path> --json '<json>'` |
| Refresh integrity sidecar | `tomlctl integrity refresh <file>` ([see sidecar files](#sidecar-files)) |

## Common recipes

```bash
# 1. Append a task-completion entry with commits[], bump last_updated
tomlctl items add .claude/flows/<slug>/execution-record.toml --json '{
  "id":"E12","type":"task-completion","task_ref":"T3",
  "timestamp":"2026-04-18T14:32:00Z","commits":["ab12cd3","9e8f1a2"]
}'
tomlctl set .claude/flows/<slug>/execution-record.toml last_updated 2026-04-18
```

```bash
# 2. Dedup-by-field add — skip if (file, summary) already present
tomlctl items add ledger.toml --dedupe-by file,summary --json '{"id":"R24",...}'
```

```bash
# 3. Mint the next id, build the payload inline, append via stdin
NEXT=$(tomlctl items next-id ledger.toml --prefix R)
printf '{"id":%s,"severity":"minor","summary":"...","status":"open"}' "\"$NEXT\"" \
  | tomlctl items add ledger.toml --json -
```

```bash
# 4. Count open items as a bare integer
tomlctl items list ledger.toml --where status=open --count --raw
```

```bash
# 5. Bulk transition — close a batch of deferred items in one parse+write
tomlctl items apply ledger.toml --ops - <<'EOF'
[
  {"op":"update","id":"R7", "json":{"status":"open"},"unset":["defer_reason","defer_trigger"]},
  {"op":"update","id":"R11","json":{"status":"open"},"unset":["defer_reason","defer_trigger"]}
]
EOF
```

## `--verify-integrity` support matrix

`--verify-integrity` is a **per-subcommand flag**, not a global — it is accepted only on read subcommands that touch a TOML + sidecar pair. Verification is rejected (clap-layer error) on every other path. See [Sidecar files](#sidecar-files) for what it checks.

| Subcommand | `--verify-integrity` |
|---|---|
| `tomlctl get` | yes |
| `tomlctl parse` | yes |
| `tomlctl validate` | yes |
| `tomlctl items list` | yes |
| `tomlctl items get` | yes |
| `tomlctl items next-id` | yes |
| `tomlctl items find-duplicates` | yes |
| `tomlctl items orphans` | yes |

`tomlctl blocks verify` intentionally does NOT accept `--verify-integrity` (it operates on markdown with no sidecar pair).

## When to use this skill

Every flow-TOML mutation routes through `tomlctl` — no Python, no line-level `Edit`, no `jq` for TOML parsing. Reach for it whenever a flow command needs to read, filter, or mutate `context.toml`, the review / optimise ledgers, or their sidecar array-of-tables (`rollback_events`, task-completion records). Shell-level post-processing of tomlctl's JSON output is not needed either — prefer in-tool primitives (`--raw` / `--lines` / `--count-distinct` / `--count`) over piping through `jq -r .count` / `jq -r '.[]'` / `| sort -u | wc -l`.

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

`tomlctl capabilities` emits a stable JSON document (`{"version":"…","features":[…],"subcommands":[…]}`) so downstream templates can feature-gate at boot without parsing `--help` prose. Features are stable within a minor release; new flags add new feature entries rather than being version-qualified. Run the command itself for the authoritative list; representative entries:

| Feature | What it enables |
|---|---|
| `count_distinct` | `--count-distinct <FIELD>` on `items list` |
| `raw` / `lines` | `--raw` / `--lines` output shapes |
| `dedupe_by` / `dedup_id_auto` | `--dedupe-by <FIELDS>` + auto-populate on every write |
| `find_duplicates_across` | `items find-duplicates --across <other>` (tier A/B) |
| `error_format_json` | `--error-format json` + `ErrorKind` taxonomy |
| `strict_read` / `dry_run` | `--strict-read` on reads / `--dry-run` on writes |
| `backfill_dedup_id` / `integrity_refresh` | legacy upgrade + sidecar regen |

## Read operations

All read commands print JSON on stdout by default.

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
tomlctl --strict-read items next-id .claude/flows/foo/review-ledger.toml --prefix R
tomlctl --strict-read items list .claude/flows/foo/review-ledger.toml --status open
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
- **`--ndjson`** — one full item per line. Pipes cleanly into `items add-many` / `items apply`.

```bash
tomlctl items list ledger.toml --status open --count --raw         # → 7
tomlctl items list ledger.toml --where id=R22 --pluck symbol --raw # → old::fn
tomlctl items list ledger.toml --status open --pluck id --lines    # R1\nR3\nR7
tomlctl items list ledger.toml --status open --ndjson              # {...}\n{...}
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

See [Advanced / maintenance](#advanced--maintenance) for `blocks verify` — infrastructure-only, no flow command invokes it.

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

For runs that need to append many new items at once (e.g. a 50-finding review batch), assemble NDJSON line-by-line and pass it to `items add-many` — one parse, one lock, one rewrite, one sidecar refresh. Each line is one JSON object; blank lines are ignored; any malformed line aborts the whole batch pre-mutation and names the offending line number.

**Always** default to the staging-file form. For any batch of **more than 5 items**, or any batch where a single row is wider than ~1 KB (typical for review/optimise findings with `summary` + `rationale` + `suggestion` prose), the staging file is the **only** supported path on Windows — the heredoc form is unreliable there (see [Stdin input for large JSON payloads](#stdin-input-for-large-json-payloads) for the failure mode and the measured threshold).

Write the NDJSON with the `Write` tool, then point `--ndjson` at the path:

```bash
tomlctl items add-many .claude/flows/foo/review-ledger.toml \
  --defaults-json '{"first_flagged":"2026-04-18","rounds":1,"status":"open"}' \
  --ndjson .claude/flows/foo/_batch.ndjson
# → {"ok":true,"added":N}
```

`--array <name>` targets a non-default array-of-tables. `--defaults-json` is optional; omit it for rows that are already fully-formed. `--dedupe-by <FIELDS>` works the same as on `items add`.

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

In `items apply` batches, an `update` op accepts a per-op `unset` array of field names alongside the `json` patch object. Both may appear on the same op: `json` sets fields, `unset` deletes fields; the `unset` pass runs **after** the `json` merge, so an `unset` on the same key as a set wins. Omitting `unset` leaves behaviour unchanged.

```json
{"op":"update","id":"R7","json":{"status":"open"},"unset":["defer_reason","defer_trigger"]}
```

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
# Same >5-item / Windows-heredoc rule as `items add-many` above. Staging file is
# mandatory on Windows for any batch larger than ~5 items.
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

All JSON-accepting flags (`--ops`, `--json` on `items add` / `items update` / `set-json`, `--defaults-json` / `--ndjson` on `items add-many` / `array-append`) treat the literal `-` as "read from stdin". Caps the read at 32 MiB, refuses to block on an interactive TTY, and allows only one `-`-consuming flag per invocation (a second errors with `stdin already consumed by another flag on this invocation`).

On Linux/macOS the heredoc form is fine for any size:

```bash
tomlctl items add-many ledger.toml --ndjson - <<'EOF'
{"id":"R1", ...}
{"id":"R2", ...}
EOF
```

**On Windows Git Bash, heredocs are unreliable — use the staging-file form for any batch of >5 items or >~10 KB.** The Bash-tool transport to Git Bash intermittently mangles the heredoc terminator (CR bytes get appended to the `EOF` delimiter), so large bodies fail with one of:

- `bash: -c: line N: unexpected EOF while looking for matching \`''` — the whole command errors out, no write happens.
- Partial success followed by spurious errors — tomlctl actually writes the first N items, then bash treats the tail of the heredoc body as shell commands to execute (e.g. `/c/Users/ros…: Permission denied`). This is the failure mode that shows up as a "false interrupt" in the UI.

Measured behaviour on this machine (Opus 4.x, Git Bash, 2026-04-24): narrow rows (a few fields, <100 bytes each) survive heredocs up to ~80 rows; typical review-finding rows (summary + file + rationale + suggestion ≈ 700 bytes) start failing intermittently at 14 rows and fail consistently at 15 rows. The practical threshold is ~10 KB of total command text. **Don't try to estimate this at call time** — just stage to a file once you're past a handful of rows.

Windows-safe pattern (mandatory for >5 items, recommended for all batches):

```bash
# 1. Write tool → .claude/flows/<slug>/_batch.ndjson  (one JSON object per line)
# 2. --ndjson <path>, no stdin, no heredoc:
tomlctl items add-many .claude/flows/<slug>/ledger.toml \
  --defaults-json '{"first_flagged":"2026-04-24","rounds":1,"status":"open"}' \
  --ndjson .claude/flows/<slug>/_batch.ndjson
# 3. Optional: rm .claude/flows/<slug>/_batch.ndjson after the call.
```

For `--json` / `--ops` / `--defaults-json` (which don't accept a file path directly), write the payload to a sibling file and pipe it in: `cat .claude/flows/<slug>/_patch.json | tomlctl … --json -`. A single-line heredoc (`<<'EOF'\n{"...":"..."}\nEOF`) is fine on Windows for one-line patches — only multi-line bodies are risky.

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

## Sidecar files

Every write produces (and every read can verify) two sidecars next to the target TOML:

- **`<file>.sha256`** — integrity sidecar in standard `sha256sum` format (`<64-lower-hex>  <basename>\n`, two spaces between digest and basename, trailing newline). Written by default on every write (atomic tempfile+rename, under the same lock as the primary write). Verified by `--verify-integrity` on reads (see the [support matrix](#--verify-integrity-support-matrix)). Regenerated by `tomlctl integrity refresh <path>`.
- **`<file>.lock`** / `.claude/.locks/<hash>.lock` — exclusive advisory lock acquired by every write path; prevents concurrent mutators from corrupting the file. On Windows this is a mandatory lock; see the lock-recovery note under [Constraints and gotchas](#constraints-and-gotchas).

```bash
# Default — writes both ledger.toml and ledger.toml.sha256
tomlctl items update ledger.toml R7 --json '{"status":"fixed"}'

# Skip the sidecar (e.g. read-only-ish FS, or hand-editing before the next write).
tomlctl items update ledger.toml R7 --json '{"status":"fixed"}' --no-write-integrity

# Treat sidecar write failures as hard errors.
tomlctl items update ledger.toml R7 --json '{"status":"fixed"}' --strict-integrity

# Verify on read — errors if sidecar is missing OR the digest disagrees.
tomlctl items list ledger.toml --where status=open --verify-integrity
```

- **Missing sidecar under `--verify-integrity`** → hard error naming the expected path; never auto-regenerated. Run `tomlctl integrity refresh` to materialise it.
- **Digest mismatch** → hard error naming both expected (from sidecar) and actual (from current bytes). Resolve by a human; `tomlctl` never auto-repairs.
- **Sidecar write failure after a successful primary write** → stderr warning and exit 0 by default (data is durable; the next write rebuilds the sidecar). `--strict-integrity` flips this to a hard error.
- **`--allow-outside`** applies identically — the sidecar lands next to the target wherever that is.

> `.sha256` is not a MAC — it detects accidental corruption and out-of-band edits, not an adversary with write access. Hostile-actor threat models still require auditing the ledger's git history.

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

## Advanced / maintenance

Infrastructure-only primitives — no flow command invokes these directly. Kept documented for hook/script authors and release-engineering work.

### `blocks verify` — shared-block parity across markdown files

`tomlctl blocks verify` checks that named shared blocks are byte-identical across a set of files, mirroring `scripts/verify-shared-blocks.sh` without the bash+awk dependency. Blocks are delimited by `<!-- SHARED-BLOCK:<name> START -->` … `<!-- SHARED-BLOCK:<name> END -->` markers (markers excluded from the hash; content between them joined by `\n`).

```bash
# Verify named blocks across the flow-command files
tomlctl blocks verify claude/commands/optimise.md claude/commands/review.md \
  claude/commands/optimise-apply.md claude/commands/review-apply.md \
  --block flow-context --block ledger-schema

# Omit --block to verify every block present in the first listed file
tomlctl blocks verify claude/commands/*.md
```

Output is JSON (`{"ok":true|false,"blocks":[...]}`); exit code 0 on success, non-zero on drift or missing markers. Does NOT accept `--verify-integrity` / `--allow-outside` / `--no-write-integrity` / `--strict-integrity` (markdown has no sidecar pair; `blocks verify` never writes).
