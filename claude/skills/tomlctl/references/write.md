# tomlctl — write reference

The mutating half of the tomlctl surface: `set`, `set-json`, `array-append`, the `items`
batch verbs (`add`, `add-many`, `update`, `remove`, `apply`, `backfill-dedup-id`) and
`integrity refresh`, together with the cross-cutting behaviours every one of them inherits
— auto-create on first write, `--dry-run` preview, stdin payload handling, and the dedup
fingerprint contract. The read-only verbs live in [query.md](query.md).

## Contents

- [Common recipes](#common-recipes)
- [Write operations](#write-operations)
  - [Auto-create on first write](#auto-create-on-first-write)
  - [Set a scalar at a path](#set-a-scalar-at-a-path)
  - [Set an array or object at a path (`set-json`)](#set-an-array-or-object-at-a-path-set-json)
  - [Append a single new item](#append-a-single-new-item)
    - [Pre-append dedup (`--dedupe-by`)](#pre-append-dedup---dedupe-by)
  - [Batch append many items — `items add-many`](#batch-append-many-items--items-add-many)
  - [Patch an existing item](#patch-an-existing-item)
    - [Unset fields](#unset-fields)
  - [Remove an item](#remove-an-item)
  - [Batch multiple mixed item ops (`items apply`)](#batch-multiple-mixed-item-ops-items-apply)
    - [Targeting a non-default array-of-tables (`--array`)](#targeting-a-non-default-array-of-tables---array)
  - [Compute the next id](#compute-the-next-id)
  - [Append to an array-of-tables — `array-append`](#append-to-an-array-of-tables--array-append)
  - [Migrate legacy ledgers — `items backfill-dedup-id`](#migrate-legacy-ledgers--items-backfill-dedup-id)
  - [Regenerate a missing sidecar — `integrity refresh`](#regenerate-a-missing-sidecar--integrity-refresh)
  - [Stdin input for large JSON payloads](#stdin-input-for-large-json-payloads)
- [Dry-run](#dry-run)
- [Dedup fingerprint contract](#dedup-fingerprint-contract)

## Common recipes

```bash
# 1. Append a task-completion entry with commits[], bump last_updated
cat <<'EOF' | tomlctl items add .claude/flows/<slug>/execution-record.toml --json -
{
  "id":"E12","type":"task-completion","task_ref":"T3",
  "timestamp":"2026-04-18T14:32:00Z","commits":["ab12cd3","9e8f1a2"]
}
EOF
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

## Write operations

Writes preserve every field the tool didn't touch, including `created`. Key order within tables is preserved.

### Auto-create on first write

Every mutating verb routed through the write chokepoint — `set`, `set-json`, `array-append`, and `items {add, add-many, apply, update, remove}` — **creates a missing target file by default** instead of erroring. On a missing file the tool seeds a starting document, then applies and persists the verb's mutation transactionally:

- **The four recognised flow files** (matched on basename: `execution-record.toml`, `review-ledger.toml`, `optimise-findings.toml`, `plan-review-findings.toml`) seed a skeleton `schema_version = 1` (TOML integer) + `last_updated = <today>` (bare date) — byte-identical to what `flow init` bootstraps.
- **Any other path** seeds an empty document (`{}`).

The seed is only the *starting* doc — the verb's mutation must still succeed against it. A no-match `update` / `remove` (or an all-update `apply`) against a freshly-seeded doc still ERRORS and leaves NO file behind: an empty seed has nothing to match, so the operation fails before the file is persisted.

**Exception — `items backfill-dedup-id` does NOT auto-create.** It pre-reads the ledger to find items lacking a `dedup_id`, so a missing target errors with `kind=not_found` regardless of `--no-create`. This is by design, not a bug: backfilling an absent ledger is a no-op, so the strict missing-file error is the correct behaviour. Every other mutating verb listed above auto-creates.

**Envelope.** Write-success envelopes now carry `"created": <bool>` and `"path": "<file>"` alongside any existing keys (e.g. `added`/`updated`/`removed`):

```bash
tomlctl items add .claude/flows/<slug>/review-ledger.toml --json '{"id":"R1","summary":"...","status":"open"}'
# {"ok":true,"created":true,"path":".claude/flows/<slug>/review-ledger.toml","added":1}
```

**Stderr guidance.** When a file is created, exactly one line is written to stderr:

- recognised flow file → `tomlctl: created new file <path> (schema_version=1)`
- any other path → `tomlctl: created new file <path>`

**`--no-create`.** Pass `--no-create` (a write-side flag) to restore the strict prior behaviour: a missing file yields `kind=not_found` and nothing is created. Use it in typo-cautious scripts that must distinguish "mutate an existing file" from "accidentally spawn a new one".

```bash
# Strict: error with kind=not_found instead of seeding a new file.
tomlctl set .claude/flows/<slug>/context.toml status review --no-create
```

> **`--allow-outside` interaction (double opt-out).** `--allow-outside` turns the `.claude/` containment guard into a no-op, and auto-create is on by default — so `--allow-outside` + a path typo can silently create a stray file ANYWHERE on disk, not just inside `.claude/`. This is a deliberate explicit double opt-out; `--no-create` is the escape hatch. Treat `--allow-outside` write paths as auto-create-capable and pair them with `--no-create` whenever the target is expected to already exist.

Not every write pipeline auto-creates: `tomlctl flow active` (the active-flow registry) already bootstraps on missing and gains no `created` field; `tomlctl json …` is unchanged (it targets `settings.json`, which always exists); and `tomlctl flow init` keeps its own created-preservation idempotency for `context.toml` + `execution-record.toml`.

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

Supports `--dry-run`; see [Dry-run](#dry-run).

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

Supports `--dry-run`; see [Dry-run](#dry-run).

### Append a single new item

`--json` takes one JSON object representing the new `[[items]]` entry. Field order in the JSON becomes field order in the emitted TOML, so pass fields in the canonical key order the `flow-contract-ledger-schema` skill defines:
`id, file, line, symbol, severity, effort, category, summary, description, evidence, first_flagged, rounds, related, status, <disposition-specific>, flow`.

```bash
cat <<'EOF' | tomlctl items add .claude/flows/foo/optimise-findings.toml --json -
{
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
}
EOF
```

`dedup_id` is auto-populated by the write funnel if the payload doesn't set it — see [Dedup fingerprint contract](#dedup-fingerprint-contract). Rendered output (e.g. PROGRESS-LOG columns) is unaffected; the field only appears in the TOML.

Date-shaped strings (`YYYY-MM-DD`) in the `DATE_KEYS` set (`created`, `updated`, `first_flagged`, `last_updated`, `resolved`, `date`) are automatically promoted to TOML date literals.

Supports `--dry-run`; see [Dry-run](#dry-run).

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

Supports `--dry-run`; see [Dry-run](#dry-run).

### Patch an existing item

Matched by `id`. The JSON object is merged into the item (shallow). Existing unmentioned fields stay untouched.

```bash
# Mark an item applied with resolution commit
cat <<'EOF' | tomlctl items update .claude/flows/foo/review-ledger.toml R22 --json -
{
  "status": "applied",
  "resolved": "2026-04-17",
  "resolution": "Fixed in ab12cd3"
}
EOF

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
tomlctl items apply ledger.toml --ops - <<'EOF'
[
  {"op":"update","id":"R7","json":{"status":"open"},"unset":["defer_reason","defer_trigger"]}
]
EOF
```

Supports `--dry-run`; see [Dry-run](#dry-run).

### Remove an item

Rare — IDs are never renumbered per spec — but occasionally needed for manual cleanup. Fails if the id does not exist.

```bash
tomlctl items remove .claude/flows/foo/review-ledger.toml R17
```

Supports `--dry-run`; see [Dry-run](#dry-run).

### Batch multiple mixed item ops (`items apply`)

For runs that mix add/update/remove on `[[items]]` in the same ledger, use `items apply` to parse + rewrite the file once. `--ops` is a JSON array; each op is `{"op": "add|update|remove", ...}` with the same payload shape as the single-op commands (`json` for add/update, `id` for update/remove). Ops run in array order; any op error aborts the whole batch and the file is left unchanged.

```bash
tomlctl items apply .claude/flows/foo/review-ledger.toml --ops - <<'EOF'
[
  {"op":"add",    "json":{"id":"R24","severity":"minor","summary":"...","status":"open"}},
  {"op":"update", "id":"R22", "json":{"status":"applied","resolved":"2026-04-17"}},
  {"op":"remove", "id":"R17"}
]
EOF
```

Prefer this over looping single-op invocations — one parse + one write instead of N. For homogeneous add-only batches prefer `items add-many` (simpler input shape). For append-only non-`items` arrays prefer `array-append`.

Supports `--dry-run`; see [Dry-run](#dry-run).

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

`--prefix` and `--infer-from-file` are mutually exclusive (one is required). `--prefix` on a missing file returns `<prefix>1` as a bootstrapping fast path (the query reference's strict-reads section covers how to disable that default). `--infer-from-file` cannot bootstrap — it needs existing items to infer from.

Returns the JSON-encoded string of the next id (prefix + `max(existing numeric suffixes) + 1`).

### Append to an array-of-tables — `array-append`

For append-only arrays such as `[[rollback_events]]` (written by `/review-apply` / `/optimise-apply` rollback protocol), use `array-append`. It's a thin shim over `items add-many` that targets an arbitrary array name and doesn't require op-type framing.

```bash
# Single record
cat <<'EOF' | tomlctl array-append <ledger> rollback_events --json -
{
  "timestamp": "2026-04-18T14:32:00Z",
  "command": "review-apply",
  "cause": "build failure",
  "items": ["R3","R7"],
  "stash_ref": "stash@{0}"
}
EOF

# Many records via NDJSON — stage to a sibling file and pass --ndjson <path>.
# Same >5-item / Windows-heredoc rule as `items add-many` above. Staging file is
# mandatory on Windows for any batch larger than ~5 items.
tomlctl array-append <ledger> rollback_events \
  --ndjson .claude/flows/foo/_rollback-batch.ndjson
```

`items apply --array <name>` remains available for heterogeneous batches (add/update/remove on the same array in one parse+write). Use `array-append` when every op is an append.

Supports `--dry-run`; see [Dry-run](#dry-run).

### Migrate legacy ledgers — `items backfill-dedup-id`

Ledgers created before 0.2.0 have no `dedup_id` field on any item. `items backfill-dedup-id` computes and writes the fingerprint for every item that lacks one, preserving any item that already has a (possibly manually set) value. Idempotent — a second run is a no-op.

```bash
# Apply (returns backfilled:0 when there's nothing to do — idempotent, write skipped)
tomlctl items backfill-dedup-id .claude/flows/foo/review-ledger.toml
# → {"ok":true,"backfilled":23}

# Kill switch engaged — short-circuits without reading the file
TOMLCTL_NO_DEDUP_ID=1 tomlctl items backfill-dedup-id <ledger>
# → {"ok":true,"backfilled":0,"reason":"disabled-by-env"}
```

Supports `--dry-run`; see [Dry-run](#dry-run).

### Regenerate a missing sidecar — `integrity refresh`

Materialises (or regenerates) the `<file>.sha256` sidecar from the file's current on-disk bytes. Does NOT modify the TOML — use this when the sidecar is absent or lost but the TOML is authoritative as-is.

No bootstrap snippet is needed any more: the first write to a missing flow file [auto-creates and seeds it](#auto-create-on-first-write) through the normal write pipeline, which produces the sidecar in the same pass — so `/plan-new` and `/implement` never have to hand-`Write` a skeleton and then run `integrity refresh` to close a sidecar gap. `integrity refresh` is now purely a recovery / regeneration verb:

```bash
# Recovery: sidecar deleted out-of-band (git clean, stray rm), TOML intact.
tomlctl integrity refresh .claude/flows/<slug>/review-ledger.toml
# → {"ok":true}
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

## Dry-run

`--dry-run` is accepted on every write subcommand — `set`, `set-json`, `array-append`,
`items add`, `items add-many`, `items update`, `items remove`, `items apply`, and
`items backfill-dedup-id`. It reports the computed mutation as a `would_change` envelope
and touches no file. The dry-run path runs the same compute stage as the real path —
mutation logic cannot drift between preview and apply.

```bash
# Preview a scalar change without touching disk (dry-run on `set`)
tomlctl set foo.toml status review --type str --dry-run
# {"ok":true,"dry_run":true,"would_change":{"kind":"scalar","path":"status","old":"draft","new":"review"}}
# Note: if the target path is an absolute path outside .claude/ (e.g. /tmp/scratch.toml),
# you must also pass --allow-outside.
```

```bash
# Preview without touching disk
tomlctl set-json .claude/flows/auth/context.toml scope --json '["src/auth/**"]' --dry-run
# {"ok":true,"dry_run":true,"would_change":{"kind":"scalar","path":"scope","old":[...],"new":["src/auth/**"]}}
```

```bash
# Preview the add without touching disk
tomlctl items add .claude/flows/foo/optimise-findings.toml --json '{...}' --dry-run
# {"ok":true,"dry_run":true,"would_change":{"kind":"items","added":1,"updated":0,"removed":0,"skipped":0,"ids":["O7"]}}
```

```bash
# Preview a batch append without touching disk
tomlctl items add-many .claude/flows/foo/review-ledger.toml --ndjson .claude/flows/foo/_batch.ndjson --dry-run
# {"ok":true,"dry_run":true,"would_change":{"kind":"items","added":N,"updated":0,"removed":0,"skipped":0,"ids":[...]}}
```

```bash
# Preview a patch without touching disk
tomlctl items update .claude/flows/foo/review-ledger.toml R22 --json '{"status":"applied"}' --dry-run
# {"ok":true,"dry_run":true,"would_change":{"kind":"items","added":0,"updated":1,"removed":0,"skipped":0,"ids":["R22"]}}
```

```bash
# Preview with --dry-run — reports the computed mutation without touching disk
tomlctl items remove .claude/flows/foo/review-ledger.toml R17 --dry-run
# → {"ok":true,"dry_run":true,"would_change":{"kind":"items","added":0,"updated":0,"removed":1,"ids":["R17"]}}
```

```bash
tomlctl items apply ledger.toml --ops '[...]' --dry-run
# → {"ok":true,"dry_run":true,"would_change":{"kind":"items","added":1,"updated":1,"removed":1,"ids":["R17","R22","R24"]}}
# Note: `ids` is the union of all affected ids (added + updated + removed combined).
```

```bash
# Preview an append without touching disk
tomlctl array-append <ledger> rollback_events --json '{...}' --dry-run
# {"ok":true,"dry_run":true,"would_change":{"kind":"items","added":1,"updated":0,"removed":0,"skipped":0,"ids":[]}}
```

```bash
# Preview
tomlctl items backfill-dedup-id .claude/flows/foo/review-ledger.toml --dry-run
# → {"ok":true,"dry_run":true,"would_backfill":23,"ids":["R1","R2",...]}
```

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
