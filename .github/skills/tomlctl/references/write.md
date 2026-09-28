# tomlctl — write reference

The mutating half of the tomlctl surface: `set`, `set-json`, `array-append`, the `items`
batch verbs (`add`, `add-many`, `update`, `remove`, `apply`, `backfill-dedup-id`,
`sweep --update`) and `integrity refresh`, together with the cross-cutting behaviours every one of them inherits
— auto-create on first write, `--dry-run` preview, stdin payload handling, and the dedup
fingerprint contract. The read-only verbs live in [query.md](query.md).

## Contents

- [Common recipes](#common-recipes)
- [Write operations](#write-operations)
  - [Auto-create on first write](#auto-create-on-first-write)
  - [`set`](#set)
  - [`set-json`](#set-json)
  - [`items add`](#items-add)
  - [`items add-many`](#items-add-many)
  - [`items update`](#items-update)
  - [`items remove`](#items-remove)
  - [`items apply`](#items-apply)
  - [`items next-id`](#items-next-id)
  - [`array-append`](#array-append)
  - [`items backfill-dedup-id`](#items-backfill-dedup-id)
  - [`items sweep --update`](#items-sweep---update)
  - [`integrity refresh`](#integrity-refresh)
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

Writes preserve every field the tool didn't touch, including `created`. Key order within tables is preserved. The write bundle every verb below carries — `--allow-outside`, `--no-create`, `--no-write-integrity`, `--strict-integrity`, `--dry-run` — is covered once, in [Auto-create on first write](#auto-create-on-first-write) and [Dry-run](#dry-run); the tables list only each verb's own flags.

### Auto-create on first write

Every mutating verb routed through the write chokepoint — `set`, `set-json`, `array-append`, and `items {add, add-many, apply, update, remove}` — **creates a missing target file by default** instead of erroring. On a missing file the tool seeds a starting document, then applies and persists the verb's mutation transactionally:

- **The four recognised flow files** (matched on basename: `execution-record.toml`, `review-ledger.toml`, `optimise-findings.toml`, `plan-review-findings.toml`) seed a skeleton `schema_version = 1` (TOML integer) + `last_updated = <today>` (bare date) — byte-identical to what `flow init` bootstraps.
- **Any other path** seeds an empty document (`{}`).

The seed is only the *starting* doc — the verb's mutation must still succeed against it. A no-match `update` / `remove` (or an all-update `apply`) against a freshly-seeded doc still ERRORS and leaves NO file behind: an empty seed has nothing to match, so the operation fails before the file is persisted.

**Exceptions — `items backfill-dedup-id` and `items sweep --update` do NOT auto-create.** Each pre-reads the ledger — the first for items lacking a `dedup_id`, the second for the `sweep` arrays it re-runs — so a missing target errors with `kind=not_found` on both regardless of `--no-create`, and `items sweep` reports `created` as `false` on every path. This is by design, not a bug: backfilling or re-sweeping an absent ledger is a no-op, so the strict missing-file error is the correct behaviour. Every other mutating verb listed above auto-creates.

**Envelope.** Write-success envelopes carry `"created": <bool>` and `"path": "<file>"` alongside any verb-specific keys (e.g. `added` on `items add-many`, `appended` on `array-append`):

```bash
tomlctl items add .claude/flows/<slug>/review-ledger.toml --json '{"id":"R1","summary":"...","status":"open"}'
# {"ok":true,"created":true,"path":".claude/flows/<slug>/review-ledger.toml"}
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

### `set`

Sets a scalar at a dotted key path.

```bash
tomlctl set .claude/flows/auth-overhaul/context.toml status review
tomlctl set .claude/flows/auth-overhaul/context.toml tasks.completed 4
tomlctl set path/to/file.toml when 2026-04-17T10:00:00Z --type datetime
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--type` | `str` \| `int` \| `float` \| `bool` \| `date` \| `datetime` | Force the value's TOML type when inference would go wrong (`42` meant as a string, a timestamp meant as a datetime). | inferred: `YYYY-MM-DD` → date, `true`/`false` → bool, digits → int, else string |

### `set-json`

Sets a non-scalar at a path — an array such as `scope`, or a whole subtable such as `[artifacts]`. ISO-date strings (`YYYY-MM-DD`) are auto-promoted to TOML date literals, same as `items add` / `items update`.

```bash
tomlctl set-json .claude/flows/auth/context.toml scope --json '["src/auth/**","src/routes/**","src/middleware/auth.rs"]'
tomlctl set-json .claude/flows/auth/context.toml artifacts --json '{"review_ledger":"x.toml","optimise_findings":"y.toml"}'
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--json` | JSON value, `-` or `@<path>` | The replacement value at the path. | required |

### `items add`

Appends one row. Pass fields in the canonical key order the `flow-contract-ledger-schema` skill defines, since JSON order becomes TOML order:
`id, file, line, symbol, severity, effort, category, summary, description, evidence, first_flagged, rounds, related, status, <disposition-specific>, flow`.

```bash
tomlctl items add .claude/flows/foo/optimise-findings.toml --json '{"id":"O7","file":"src/svc/foo.rs","line":44,"severity":"critical","effort":"small","category":"memory","summary":"Allocates fresh Vec in hot loop","first_flagged":"2026-04-17","rounds":1,"status":"open"}'
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--json` | JSON object, `-` or `@<path>` | The new row. | required |
| `--array` | name | Target array-of-tables, e.g. `rollback_events`. | `items` |
| `--dedupe-by` | `f1,f2,…` | Skip the add when an existing row equals the payload on every listed field, compared as raw strings; the envelope then reports `"added":0` and the `matched_id`. `dedup_id` is never implied — name it for fingerprint dedup. The pre-scan runs before `dedup_id` auto-populates, so a payload's own fingerprint never matches itself. | off |

`dedup_id` is auto-populated by the write funnel if the payload doesn't set it — see [Dedup fingerprint contract](#dedup-fingerprint-contract). Rendered output (e.g. PROGRESS-LOG columns) is unaffected; the field only appears in the TOML.

Date-shaped strings (`YYYY-MM-DD`) in the `DATE_KEYS` set — `created`, `updated`, `first_flagged`, `last_updated`, `resolved`, `date`, `promoted`, `dismissed`, `last_seen` — are automatically promoted to TOML date literals. The `DATE_KEYS` constant in `tomlctl/src/convert.rs` owns the set; check it there when a field you expected to promote stayed a quoted string.

### `items add-many`

Appends many rows — e.g. a 50-finding review batch — in one parse, one lock, one rewrite, one sidecar refresh.

Two agent-native forms, neither a heredoc and neither a temp file. Prefer the pipe for a handful of rows and the staging file past that (see [Stdin input for large JSON payloads](#stdin-input-for-large-json-payloads) for why the heredoc form is not on this list).

**Pipe, one row per argument** — a single-line command; `printf '%s\n'` emits each argument on its own line, so the rows arrive as NDJSON without a heredoc:

```bash
printf '%s\n' '{"id":"R1","summary":"..."}' '{"id":"R2","summary":"..."}' | tomlctl items add-many ledger.toml --ndjson - --defaults-json '{"status":"open"}'
```

The PowerShell spelling pipes a string array; each element becomes one line:

```powershell
'{"id":"R1","summary":"..."}','{"id":"R2","summary":"..."}' | tomlctl items add-many ledger.toml --ndjson - --defaults-json '{"status":"open"}'
```

**Staging file** — write the NDJSON with the `Write` tool, then point `--ndjson` at the path:

```bash
tomlctl items add-many .claude/flows/foo/review-ledger.toml \
  --defaults-json '{"first_flagged":"2026-04-18","rounds":1,"status":"open"}' \
  --ndjson .claude/flows/foo/_batch.ndjson
# → {"ok":true,"added":N,"created":false,"path":".claude/flows/foo/review-ledger.toml"}
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--ndjson` | `-` or path (a leading `@` is accepted) | One JSON object per line; blank lines are ignored, and a malformed line aborts the whole batch before mutation, naming its line number. | required |
| `--defaults-json` | JSON object, `-` or `@<path>` | Fields stamped on every row; a row's own key wins. Omit for fully-formed rows; `@defaults.json` pairs with `--ndjson -`. | none |
| `--array` | name | As on `items add`. | `items` |
| `--dedupe-by` | `f1,f2,…` | As on `items add`, per row; skipped rows add `"skipped":M` and `"skipped_rows":[{"row":N,"matched_id":"…"}]` to the envelope. | off |

### `items update`

Patches the row whose `id` matches. The patch is a shallow merge; unmentioned fields stay untouched.

```bash
tomlctl items update .claude/flows/foo/review-ledger.toml R22 --json '{"status":"applied","resolved":"2026-04-17","resolution":"Fixed in ab12cd3"}'

# Flip deferred -> open and drop the defer triggers in a single rewrite
tomlctl items update ledger.toml R7 --json '{"status":"open","rounds":2}' --unset defer_reason --unset defer_trigger
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--json` | JSON object, `-` or `@<path>` | The patch. At least one of `--json` / `--unset` is required. | none |
| `--unset` | key, repeatable | Drop a field. Runs after the `--json` merge, so it wins over a same-key set; a key the row lacks is a no-op. | none |
| `--array` | name | As on `items add`. | `items` |

`dedup_id` is recomputed by the write funnel when the patch touches a fingerprinted field (`file`, `summary`, `severity`, `category`, `symbol`) and does not set `dedup_id` explicitly. See [Dedup fingerprint contract](#dedup-fingerprint-contract).

### `items remove`

Rare — IDs are never renumbered per spec — but occasionally needed for manual cleanup. Fails if the id does not exist.

```bash
tomlctl items remove .claude/flows/foo/review-ledger.toml R17
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--array` | name | As on `items add`. | `items` |

### `items apply`

Runs a mixed add/update/remove batch against one array in a single parse + rewrite. Each op is `{"op": "add|update|remove", ...}` with the single-op payload shape: `json` for add/update (the patch key is `json`, not `set`), `id` for update/remove, and on update an optional `unset` array of field names applied after the `json` merge, as `--unset` is. Ops run in order; any op error aborts the whole batch and the file is left unchanged.

```bash
tomlctl items apply .claude/flows/foo/review-ledger.toml --ops - <<'EOF'
[
  {"op":"add",    "json":{"id":"R24","severity":"minor","summary":"...","status":"open"}},
  {"op":"update", "id":"R22", "json":{"status":"applied","resolved":"2026-04-17"},"unset":["defer_reason"]},
  {"op":"remove", "id":"R17"}
]
EOF

printf '%s\n' '{"op":"update","id":"R22","json":{"status":"applied"}}' '{"op":"remove","id":"R17"}' | tomlctl items apply .claude/flows/foo/review-ledger.toml --ops -
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--ops` | JSON array or NDJSON, `-` or `@<path>` | The batch. A payload whose first non-whitespace character is `{` is read as NDJSON, one op per line: blank lines are skipped and a malformed line aborts the batch naming its 1-based line number. | required |
| `--array` | name | Run the batch against another array-of-tables, e.g. `rollback_events`. | `items` |
| `--no-remove` | — | Reject any `remove` op. The apply flows pass it so an agent-generated payload cannot erase audit history. | off |

Prefer this over looping single-op invocations — one parse + one write instead of N. For homogeneous add-only batches prefer `items add-many` (simpler input shape). For append-only non-`items` arrays prefer `array-append`.

`items next-id`, `items find-duplicates`, `items orphans`, `items sweep`, and `items clusters` take no `--array`: they are ledger-schema specific and only reason about `[[items]]`.

### `items next-id`

Returns the JSON-encoded next id: the prefix + `max(existing numeric suffixes) + 1`.

```bash
tomlctl items next-id .claude/flows/foo/review-ledger.toml --prefix R        # → "R23"
tomlctl items next-id .claude/flows/foo/review-ledger.toml --infer-from-file # → "R23"
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--prefix` | letter | The id prefix. On a missing file returns `<prefix>1` as a bootstrapping fast path; [strict reads](query.md#strict-reads---strict-read) disable that. Required unless `--infer-from-file` is given. | — |
| `--infer-from-file` | — | Take the prefix from the existing ids. Cannot bootstrap: errors on an empty ledger (`--infer-from-file requires a non-empty ledger or explicit --prefix`) and on more than one prefix (`--infer-from-file found multiple prefixes (R, O); pass --prefix explicitly`). Excludes `--prefix`. | off |

### `array-append`

Appends records to an arbitrary array-of-tables such as `[[rollback_events]]` (written by the `/review-apply` / `/optimise-apply` rollback protocol). A thin shim over `items add-many` that takes the array name positionally and needs no op framing. The envelope reports `appended`.

```bash
tomlctl array-append <ledger> rollback_events --json '{"timestamp":"2026-04-18T14:32:00Z","command":"review-apply","cause":"build failure","items":["R3","R7"],"stash_ref":"stash@{0}"}'
tomlctl array-append <ledger> rollback_events --ndjson .claude/flows/foo/_rollback-batch.ndjson
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--json` | JSON object, `-` or `@<path>` | One record. Exactly one of `--json` / `--ndjson` is required. | — |
| `--ndjson` | `-` or path (a leading `@` is accepted) | Many records, one per line — the same staging-file rule as `items add-many`. | — |

`items apply --array <name>` remains available for heterogeneous batches (add/update/remove on the same array in one parse+write). Use `array-append` when every op is an append.

### `items backfill-dedup-id`

Computes and writes the fingerprint for every item that lacks a `dedup_id`, preserving any item that already has a (possibly manually set) value — the upgrade path for legacy ledgers written before the field existed. Idempotent: a second run reports `backfilled:0` and skips the write. Under the [rollback lever](#dedup-fingerprint-contract) it short-circuits without reading the file.

```bash
tomlctl items backfill-dedup-id .claude/flows/foo/review-ledger.toml
# → {"ok":true,"backfilled":23,"created":false,"path":".claude/flows/foo/review-ledger.toml"}
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--array` | name | Backfill another array-of-tables that carries a `dedup_id` contract. | `items` |

### `items sweep --update`

`items sweep <ledger>` is read-only by default — it re-runs each item's stored `sweep` patterns and reports `new` / `gone` / `kept` / `unverified` against `instances`; its flag table is [`items sweep`](query.md#items-sweep) in query.md. `--update` turns that same run into one write: a single `MutationPlan` carrying one `update` op per swept item whose `instances` actually changes, applied in one parse + one rewrite. The rewritten `instances` keeps the listed order: the kept anchors and any `excluded` ones stay in place, spelled as recorded (a `file:symbol` anchor keeps its symbol), and the new sites follow as `file:line`; an item whose rewritten list equals what is stored gets no op. Only an item whose `status` is `open` (absent or unrecognised reads as `open`) gets an op: a terminal item (`fixed` / `wontfix` / `verified-clean` / `deferred` / `applied` / `wontapply`) is reported but never rewritten, even when named in `--ids` — edit its `instances` with `items update`. `enumeration` is never rewritten — whether the patterns can reach every form is the agent's judgement, and the read output reports `coverage_complete` instead.

```bash
# Rewrite instances for two items; a run that changes nothing writes nothing and leaves the sidecar alone
tomlctl items sweep .claude/flows/foo/review-ledger.toml --ids R5,R9 --update
# → {"ok":true,"updated":["R5"],"created":false,"path":".claude/flows/foo/review-ledger.toml"}
```

`--update` refuses with `kind=validation` when any open swept item is `truncated` or has an `unverified` anchor whose reason is not `excluded` — absence of a hit in a file the sweep did not search is not evidence the anchor is gone. The refusal names up to five anchors per id with their reason plus the total, and advises raising `--max-hits` for a truncated run or re-anchoring / resolving the named anchors by hand otherwise (`--max-file-bytes` for a `skipped` oversize file); then re-run. `--dry-run` without `--update` is refused (`kind=other`): the read-only sweep has nothing to preview.

A changed run refreshes the `.sha256` sidecar like every other write; a no-change run reports `updated: []` and touches neither. A missing ledger is `kind=not_found` on every `items sweep` path — read-only, `--update` and `--update --dry-run` alike — because an empty seed would have no items to rewrite; `--update` never mints one, `--no-create` changes nothing here, and `created` is always `false`.

### `integrity refresh`

Materialises (or regenerates) the `<file>.sha256` sidecar from the file's current on-disk bytes. Does NOT modify the TOML — use this when the sidecar is absent or lost but the TOML is authoritative as-is. The first write to a missing flow file [auto-creates it](#auto-create-on-first-write) together with its sidecar, so this is a recovery verb only:

```bash
# Recovery: sidecar deleted out-of-band (git clean, stray rm), TOML intact.
tomlctl integrity refresh .claude/flows/<slug>/review-ledger.toml
# → {"ok":true}
```

Acquires the same exclusive lock a write path would, so it serialises correctly with concurrent writers. Subject to the same `.claude/` containment guard as other write paths — pass `--allow-outside` to refresh a sidecar for a file outside `.claude/`. Calling this on a file that already has a valid sidecar rewrites the same bytes, so it is idempotent.

### Stdin input for large JSON payloads

All JSON-accepting flags (`--ops`, `--json` on `items add` / `items update` / `set-json`, `--defaults-json` / `--ndjson` on `items add-many` / `array-append`) take three spellings: a literal, `-` for stdin, or `@<path>` to read a file. Stdin is capped at 32 MiB and a payload past the cap is an error, never a silent partial batch; tomlctl refuses to block on an interactive TTY; and only one flag per invocation may be `-`, because a process has one stdin (a second errors with `stdin already consumed by another flag on this invocation`). `@<path>` is the release valve — `--ndjson - --defaults-json @defaults.json`, or `--ops @ops.json` for a batch of edits with no pipe at all. In PowerShell quote it (`'@ops.json'`); a bare `@name` is splatting syntax there.

The single-line pipe (`printf '%s\n' '<row>' '<row>' | tomlctl … --ndjson -`, or a PowerShell string array) carries any number of rows without a heredoc or a file; see [`items add-many`](#items-add-many) for both spellings.

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

The threshold is roughly 10 KB of total command text, which a batch of typical review-finding rows passes at around a dozen. **Don't try to estimate this at call time** — just stage to a file once you're past a handful of rows.

Windows-safe pattern (mandatory for >5 items, recommended for all batches):

```bash
# 1. Write tool → .claude/flows/<slug>/_batch.ndjson  (one JSON object per line)
# 2. --ndjson <path>, no stdin, no heredoc:
tomlctl items add-many .claude/flows/<slug>/ledger.toml \
  --defaults-json '{"first_flagged":"2026-04-24","rounds":1,"status":"open"}' \
  --ndjson .claude/flows/<slug>/_batch.ndjson
# 3. Optional: rm .claude/flows/<slug>/_batch.ndjson after the call.
```

For `--json` / `--ops` / `--defaults-json`, write the payload to a sibling file and pass it as `@.claude/flows/<slug>/_patch.json` — no pipe, no heredoc. A single-line heredoc (`<<'EOF'\n{"...":"..."}\nEOF`) is fine on Windows for one-line patches — only multi-line bodies are risky.

## Dry-run

`--dry-run` is accepted on every write subcommand — `set`, `set-json`, `array-append`,
`items add`, `items add-many`, `items update`, `items remove`, `items apply`,
`items backfill-dedup-id`, and `items sweep --update` (there it requires `--update`, since
the read-only sweep has nothing to preview). It reports the computed mutation and touches no
file or sidecar. The dry-run path runs the same compute stage as the real path — mutation
logic cannot drift between preview and apply. A target outside `.claude/` still needs
`--allow-outside`.

The envelope takes three shapes. `set` and `set-json` report `kind: "scalar"` with the old and
new value at the path. Every `items` verb, `array-append` and `items sweep --update` report
`kind: "items"` with per-op counts and `ids`, the union of every affected id — an
`array-append` record carries no `id`, so each shows as `""`. `items backfill-dedup-id`
reports `would_backfill` and the ids it would stamp.

```bash
tomlctl set foo.toml status review --dry-run
# {"ok":true,"dry_run":true,"would_change":{"kind":"scalar","path":"status","old":"draft","new":"review"}}

tomlctl items apply ledger.toml --ops '[...]' --dry-run
# {"ok":true,"dry_run":true,"would_change":{"kind":"items","added":1,"updated":1,"removed":1,"skipped":0,"ids":["R24","R22","R17"]}}

tomlctl items backfill-dedup-id ledger.toml --dry-run
# {"ok":true,"dry_run":true,"would_backfill":23,"ids":["R1","R2",...]}
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

`--dedupe-by` on `items add` / `items add-many` is a separate, caller-chosen pre-scan; see [`items add`](#items-add).
