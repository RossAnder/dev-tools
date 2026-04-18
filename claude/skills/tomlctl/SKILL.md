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
- Read, filter, project, group, sort, or count `[[items]]` in `review-ledger.toml` / `optimise-findings.toml`.
- Update a single scalar (`status`, `updated`, `tasks.completed`) in `context.toml`.
- Append one or many new `[[items]]` entries, or patch fields on an existing item by `id`.
- Append a record to a non-`items` array-of-tables (e.g. `[[rollback_events]]`).
- Compute the next `R{n}` / `O{n}` id.

Every flow-TOML mutation routes through `tomlctl` — no Python, no line-level `Edit`, no `jq` pipelines.

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

## Read operations

All read commands print JSON on stdout.

```bash
# Whole document as JSON (omit the path argument to read the entire file)
tomlctl get .claude/flows/auth-overhaul/context.toml

# Single value at a dotted path
tomlctl get .claude/flows/auth-overhaul/context.toml status
tomlctl get .claude/flows/auth-overhaul/context.toml tasks.completed
tomlctl get .claude/flows/auth-overhaul/context.toml artifacts.optimise_findings

# Parse-check (exit 0 on valid)
tomlctl validate .claude/flows/auth-overhaul/context.toml
```

TOML dates render as ISO-8601 strings in the JSON output.

`tomlctl parse <file>` remains accepted as a deprecated alias for `tomlctl get <file>` (no path argument) — kept for backward compatibility with older scripts. Prefer `tomlctl get <file>` in new docs and recipes.

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

# Bucket by a field, emit counts
tomlctl items list ledger.toml --count-by status
# → {"open": 7, "fixed": 12, "wontfix": 1}

# Bucket by a field, emit item lists
tomlctl items list ledger.toml --group-by file
# → {"src/a.rs": [item, ...], "src/b.rs": [item, ...]}
```

`--count`, `--count-by`, and `--group-by` are mutually exclusive.

### NDJSON output

```bash
# Newline-delimited one-per-line output — pipes cleanly into items add-many / apply
tomlctl items list ledger.toml --status open --ndjson
```

### Single-item fetch

```bash
tomlctl items get .claude/flows/auth-overhaul/review-ledger.toml R22
```

### Find duplicates (read-only)

`tomlctl items find-duplicates <ledger> [--tier A|B|C]` surfaces likely-duplicate items without touching the ledger. Output is a JSON array of `{tier, key, items}` groups (empty array when no duplicates).

```bash
# Tier A (default): canonical dedup rule — group by (file, symbol) when
# symbol is non-empty, otherwise by (file, summary).
tomlctl items find-duplicates ledger.toml

# Tier B: content fingerprint (suggest-not-auto). Groups items sharing
# <file>|<summary>|<severity>|<category>|<symbol> (truncated SHA-256, 16 hex)
# and the same file basename.
tomlctl items find-duplicates ledger.toml --tier B

# Tier C: line-window for symbol-less items — items with the same file grouped
# by a greedy line window. A group extends from its minimum-line item as long
# as max(line) - min(line) <= 10; once an item exceeds the window from the
# group's min-line anchor, a new group starts at that item.
tomlctl items find-duplicates ledger.toml --tier C
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

Date-shaped strings (`YYYY-MM-DD`) in the `DATE_KEYS` set (`created`, `updated`, `first_flagged`, `last_updated`, `resolved`, `date`) are automatically promoted to TOML date literals.

### Batch append many items — `items add-many`

For runs that need to append many new items at once (e.g. a 50-finding review batch), assemble NDJSON line-by-line in bash and pipe to `items add-many`. Each line is one JSON object; blank lines are ignored; any malformed line aborts the whole batch pre-mutation and names the offending line number.

```bash
# Common fields stamped once via --defaults-json; per-row keys win on conflict.
tomlctl items add-many .claude/flows/foo/review-ledger.toml \
  --defaults-json '{"first_flagged":"2026-04-18","rounds":1,"status":"open"}' \
  --ndjson - <<'EOF'
{"id":"R1","file":"src/a.rs","line":10,"severity":"minor","category":"quality","summary":"…"}
{"id":"R2","file":"src/b.rs","line":22,"severity":"major","category":"quality","summary":"…"}
{"id":"R3","file":"src/c.rs","line":7,"severity":"critical","category":"memory","summary":"…"}
EOF
# → {"ok":true,"added":3}
```

`--ndjson <path>` reads from a file instead of stdin. `--array <name>` targets a non-default array-of-tables. `--defaults-json` is optional; omit it for rows that are already fully-formed.

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
```

### Compute the next id

```bash
tomlctl items next-id .claude/flows/foo/review-ledger.toml --prefix R   # → "R23"
tomlctl items next-id .claude/flows/foo/optimise-findings.toml --prefix O  # → "O1" on empty
```

Returns the JSON-encoded string of the next id (prefix + `max(existing numeric suffixes) + 1`). Empty ledger → `<prefix>1`.

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

# Many records via NDJSON
tomlctl array-append <ledger> rollback_events --ndjson - <<'EOF'
{"timestamp":"…","command":"…","cause":"…","items":["…"]}
{"timestamp":"…","command":"…","cause":"…","items":["…"]}
EOF
```

`items apply --array <name>` remains available for heterogeneous batches (add/update/remove on the same array in one parse+write). Use `array-append` when every op is an append.

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

#### Targeting a non-default array-of-tables (`--array`)

`items apply` defaults to mutating the `[[items]]` array at the ledger root. Pass `--array <name>` to redirect the batch at a different array-of-tables (e.g. `rollback_events`). The flag is apply-only (not accepted on `items add|update|remove|list|get`).

### Stdin input for large JSON payloads

All JSON-accepting flags (`--ops`, `--json` on `items add` / `items update` / `set-json`, `--defaults-json` / `--ndjson` on `items add-many` / `array-append`) treat the literal value `-` as "read from stdin". Use this to avoid shell-quoting or tempfile round-trips when the payload is large or contains quotes / newlines / dollar signs.

Stdin consumption rules:
- Refuses to block on an interactive TTY (so `… --json -` without a pipe errors fast rather than hanging).
- Caps the read at 32 MiB.
- Only one flag per invocation may use `-` (reading the same stdin twice returns an empty payload).

## Integrity sidecar

Every write emits an integrity sidecar next to the target: `<file>.sha256`, in the standard `sha256sum` format (`<64-lower-hex>  <basename>\n`, two spaces between the digest and the basename, trailing newline). The sidecar is written atomically via tempfile + rename under the same lock as the primary write, so an interleaved `--verify-integrity` reader never sees a torn pair.

**Threat model.** The sidecar is a consistency check against accidental corruption and collaborative out-of-band edits — it is **not** a MAC or tamper-proof signature. An attacker with ledger write access can trivially rewrite the sidecar; hostile-actor threat models still require auditing the ledger's git history.

```bash
# Default behaviour: write writes both ledger.toml and ledger.toml.sha256
tomlctl items update ledger.toml R7 --json '{"status":"fixed"}'

# Skip the sidecar for this invocation (e.g. read-only-ish filesystems or
# when you want to hand-edit before the next write regenerates it).
tomlctl --no-write-integrity items update ledger.toml R7 --json '{"status":"fixed"}'
```

Pass `--verify-integrity` on any invocation to verify the target against its sidecar before every read. Wires into `parse`, `get`, `validate`, `items list`, `items get`, `items next-id`, `items find-duplicates`, `items orphans`.

```bash
tomlctl --verify-integrity items list ledger.toml --status open
# If the sidecar is missing OR the digest disagrees, the command exits
# non-zero and the error names both hashes + the sidecar path.
```

- **Missing sidecar with `--verify-integrity`** → error names the expected sidecar path; never auto-regenerated.
- **Digest mismatch** → error names both the expected (from sidecar) and actual (from current file bytes) digests. Resolve by a human (either the file drifted out-of-band or the sidecar is stale). `tomlctl` will never auto-repair.
- **Sidecar write failure** (disk full, permissions, etc.) after the primary write has landed → logged to stderr as a warning, but the command still exits 0 — the data is durable, the sidecar can be rebuilt by any subsequent write.
- **Same write-guard applies** — sidecars are written alongside the target, so a write that passed `--allow-outside` writes its sidecar to the same location.

## Constraints and gotchas

- **No comment preservation.** The schemas forbid inline comments, so this is fine for flow/ledger files. Do not point `tomlctl` at TOML files where comments matter.
- **Whole-file rewrite.** Any write operation reparses, mutates, and re-serialises the whole document. Never runs a line-level Edit.
- **Whitespace may change.** Long inline arrays may be reflowed to multi-line by the serializer. Semantically identical.
- **`created` is preserved verbatim.** The tool never touches it unless you explicitly `set created <date>` (don't).
- **Unknown-value rules stay with the caller.** `tomlctl` returns raw values; the command's "unknown status → treat as in-progress" / "unknown category → fail-soft" rules apply in the calling command's logic, not in the tool.
- **Errors exit non-zero and print to stderr.** Success paths emit either JSON data or `{"ok":true,…}` to stdout. Always check exit code in scripted flows.
- **Lock timeout: 30 seconds.** Writes acquire an exclusive OS-level lock on a sidecar `.lock` file next to the target (e.g. `review-ledger.toml.lock`). `tomlctl` polls `try_lock_exclusive` every 500 ms and bails after 30 s total with an error naming the lock path. On Windows this is a mandatory lock — a crashed or stuck `tomlctl` leaves the `.lock` file present and the OS keeps the lock until the offending process dies. **Recovery when a lock is stranded:** confirm no live `tomlctl` process holds it (Task Manager / `Get-Process tomlctl` / `ps aux | grep tomlctl`), then delete the `<target>.lock` file. The next invocation will recreate and re-acquire it cleanly.
- **Write-path safety (best-effort containment guard, not a sandbox).** Write operations (`set`, `set-json`, `items add|update|remove|apply|add-many`, `array-append`) reject targets that canonicalise outside the current repo's `.claude/` directory. The guard resolves symlinks and `..` at canonicalisation time and rejects paths not under `<git-top-level>/.claude/`. Read operations are not guarded. Pass `--allow-outside` (a per-subcommand flag) to override when you genuinely need to edit a flow TOML elsewhere — e.g. `tomlctl set /tmp/scratch.toml status draft --allow-outside`. `--allow-outside` is pinned behind an interactive permission prompt at the project settings level — it should never appear in unattended automation. Treat this as a best-effort guard against agent/user typos that would otherwise land writes in unintended locations; it is not a security sandbox and a TOCTOU-race or symlink swap between canonicalisation and open can in principle escape it.

## Permissions

`Bash(tomlctl *)` is pre-approved in the project's `.claude/settings.json`. `Bash(tomlctl --allow-outside *)` is explicitly denied at the same layer, so any invocation passing `--allow-outside` falls through to an interactive permission prompt. Agents should never emit `--allow-outside` unattended — the write-path containment guard is default-on for a reason.
