# tomlctl — write reference

The mutating half of the tomlctl surface: `set`, `set-json`, `array-append`, the `items`
batch verbs (`add`, `add-many`, `update`, `remove`, `apply`, `backfill-dedup-id`,
`sweep --update`) and `integrity refresh`, together with the cross-cutting behaviours every one of them inherits
— auto-create on first write, `--dry-run` preview, the field flags, payload input, and the dedup
fingerprint contract. The read-only verbs live in [query.md](query.md); execution-record
entries are written with `flow record`, documented in [flow.md](flow.md).

## Contents

- [Common recipes](#common-recipes)
- [Write operations](#write-operations)
  - [Auto-create on first write](#auto-create-on-first-write)
  - [`last_updated` stamping](#last_updated-stamping)
  - [Field flags](#field-flags)
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
# 1. Record a task completion: the tool mints the id, stamps the date and derives task_ref
#    from task 7. The summary was written to a file with the Write tool first.
tomlctl flow record --slug <slug> --type task-completion --task 7 \
  --set agent=implement --set dispatch_tier=lite --set dispatch_agent=implement-lite \
  --set vet=sampled-pass --set retries=0 --set status=done \
  --set-file summary=.claude/flows/<slug>/_summary.txt \
  --set-json 'files=["src/retry.rs"]' --set-json 'commits=["ab12cd3","9e8f1a2"]'
```

```bash
# 2. Dedup-by-field add from field flags — skip if (file, summary) already present
tomlctl items add ledger.toml --dedupe-by file,summary --id-prefix R --set file=src/a.rs --set summary="..." --set status=open
```

```bash
# 3. Mint the id inside the locked write and print it bare (→ R23)
tomlctl items add ledger.toml --id-prefix R --get id --json '{"severity":"minor","summary":"...","status":"open"}'
```

```bash
# 4. Count open items as a bare integer
tomlctl items list ledger.toml --where status=open --count --raw
```

```bash
# 5. Bulk transition — reopen a batch of deferred items in one parse+write
printf '%s\n' '{"op":"update","id":"R7","json":{"status":"open"},"unset":["defer_reason","defer_trigger"]}' '{"op":"update","id":"R11","json":{"status":"open"},"unset":["defer_reason","defer_trigger"]}' | tomlctl items apply ledger.toml --ops -
```

```bash
# 6. Update a flow context in one write; its root `updated` refreshes with it
tomlctl set .claude/flows/<slug>/context.toml --set status=in-progress --set tasks.completed=4
```

## Write operations

Writes preserve every field the tool didn't touch, including `created`. Key order within tables is preserved. The write bundle every verb below carries — `--allow-outside`, `--no-create`, `--no-write-integrity`, `--strict-integrity`, `--no-stamp`, `--dry-run` — is covered once, in [Auto-create on first write](#auto-create-on-first-write), [`last_updated` stamping](#last_updated-stamping) and [Dry-run](#dry-run); the tables list only each verb's own flags.

### Auto-create on first write

Every mutating verb routed through the write chokepoint — `set`, `set-json`, `array-append`, and `items {add, add-many, apply, update, remove}` — **creates a missing target file by default** instead of erroring. On a missing file the tool seeds a starting document, then applies and persists the verb's mutation transactionally:

- **The recognised flow files** (matched on basename: `execution-record.toml`, `review-ledger.toml`, `optimise-findings.toml`, `plan-review-findings.toml`, `backlog.toml`, `tasks.toml`, `agents.toml`) seed a skeleton `schema_version = 1` (TOML integer) + `last_updated = <today>` (bare date) — byte-identical to what `flow init` bootstraps. So does any `.toml` directly inside a flow-less ledger directory — `.claude/reviews/`, `.claude/optimise-findings/`, `.claude/plan-review-findings/` — whatever its basename, so those ledgers carry a `last_updated` for [stamping](#last_updated-stamping) to refresh.
- **Any other path** seeds an empty document (`{}`).

The seed is only the *starting* doc — the verb's mutation must still succeed against it. A no-match `update` / `remove` (or an all-update `apply`) against a freshly-seeded doc still ERRORS and leaves NO file behind: an empty seed has nothing to match, so the operation fails before the file is persisted.

**Exceptions — `items backfill-dedup-id` and `items sweep --update` do NOT auto-create.** Each pre-reads the ledger — the first for items lacking a `dedup_id`, the second for the `sweep` arrays it re-runs — so a missing target errors with `kind=not_found` on both regardless of `--no-create`, and `items sweep` reports `created` as `false` on every path. This is by design, not a bug: backfilling or re-sweeping an absent ledger is a no-op, so the strict missing-file error is the correct behaviour. Every other mutating verb listed above auto-creates.

**Envelope.** Write-success envelopes carry `"created": <bool>` and `"path": "<file>"` alongside any verb-specific keys (e.g. `added` on `items add` and `items add-many`, `appended` on `array-append`):

```bash
tomlctl items add .claude/flows/<slug>/review-ledger.toml --id-prefix R --json '{"summary":"...","status":"open"}'
# {"ok":true,"added":1,"id":"R1","created":true,"path":".claude/flows/<slug>/review-ledger.toml"}
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

### `last_updated` stamping

`set`, `set-json`, `array-append` and `items {add, add-many, update, remove, apply, backfill-dedup-id, sweep --update}` refresh the root `last_updated` to today's date (UTC) as part of the same write — there is no second `set … last_updated` call to make. The rule:

- **Existing key only.** A file whose root has no `last_updated` never gains one; recognised flow files and flow-less ledgers are [seeded](#auto-create-on-first-write) with it.
- **Changed writes only.** A write that changes nothing — a dedupe-skipped add, a `backfill-dedup-id` or `sweep --update` run with nothing to do — leaves the file and its date alone. A `--dry-run` preview is never stamped.
- **The caller's own value wins.** `set <file> last_updated 2026-04-18` (or `set-json` on that key) keeps that date.
- **`--no-stamp`** leaves `last_updated` as it is. Use it for an interim checkpoint write that must not mark the ledger fresh before the run completes.

```bash
tomlctl items update ledger.toml R7 --json '{"status":"deferred"}' --no-stamp
```

**`updated` on a flow context.** On a file named `context.toml`, `set`, `set-json` and `array-append` also refresh an existing root `updated` to today, under the same three rules: only when the root already has the key, only when the write changed something else, and never over an `updated` the write sets itself. `--no-stamp` suppresses both stamps. No other file's `updated` is touched.

The `tasks`, `backlog`, `inputs` and `agents` writes stamp their own stores and take no `--no-stamp`. The library functions glimpse links never stamp.

### Field flags

`items add`, `items update`, `array-append` and [`flow record`](flow.md) build their JSON object from three repeatable flags, so a single entry needs no hand-built JSON. `set` takes `--set` alone, with its own typing rules (see [`set`](#set)).

| Flag | Value | Meaning |
|---|---|---|
| `--set` | `KEY=VALUE` | The value is always a JSON string. A [`DATE_KEYS`](#items-add) key still lands as a TOML date, as it would from `--json`. |
| `--set-json` | `KEY=JSON` | Any JSON value: a number, a bool, an array or an object. |
| `--set-file` | `KEY=PATH` | The UTF-8 text of the file, with one leading byte-order mark and one trailing newline (`\n` or `\r\n`) stripped; every other byte is kept. `-` reads stdin, under the same one-`-`-per-invocation rule and 32 MiB cap as a `--json -` payload. |

- Each flag splits on its first `=`, so a value may itself contain `=`.
- A dotted KEY nests: `--set meta.owner=ross` writes `{"meta":{"owner":"ross"}}`.
- One KEY named twice, by the same flag or across the three, is a `kind=validation` error.
- The flags merge over an optional `--json` base object, nested objects included, and the flags win. With a field flag present, `--json` is optional.

```bash
tomlctl items update ledger.toml R7 --set status=wontfix --set-file wontfix_rationale=.claude/flows/<slug>/_rationale.md
tomlctl items add ledger.toml --id-prefix R --json '{"status":"open","rounds":1}' --set severity=minor --set-json 'line=44' --set-file summary=.claude/flows/<slug>/_summary.txt
```

**Prose goes through `--set-file`.** Write the text to a file with the Write tool and name it with `--set-file`: nothing is escaped by hand, quotes and newlines survive byte for byte, and the file path needs no quoting in either shell (PowerShell drops an unquoted `@path`, but not a `KEY=PATH` value).

**Git Bash rewrites a leading `/`.** MSYS path conversion turns a `--set` value that starts with `/` into a Windows path: `--set summary=/implement …` arrives as `summary=C:/Program Files/Git/implement …`. Pass such a value through `--set-file`, or prefix the call with `MSYS_NO_PATHCONV=1`.

### `set`

Sets scalars at dotted key paths, in one write: the positional `PATH VALUE` pair and every `--set PATH=VALUE`. Both forms infer the type the same way; the positionals are optional when a `--set` is given. Update a flow's `context.toml` with one `set`, not one call per key:

```bash
tomlctl set .claude/flows/auth-overhaul/context.toml status review --set tasks.completed=4 --set tasks.in_progress=1
tomlctl set path/to/file.toml when 2026-04-17T10:00:00Z --type datetime
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--type` | `str` \| `int` \| `float` \| `bool` \| `date` \| `datetime` | Force the positional pair's TOML type when inference would go wrong (`42` meant as a string, a timestamp meant as a datetime). It never applies to a `--set` pair. | inferred: `YYYY-MM-DD` → date, `true`/`false` → bool, digits → int, else string |
| `--set` | `PATH=VALUE`, repeatable | Another scalar in the same write, split on the first `=`. A path given twice, here or as the positional, is an error and nothing is written. | none |

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

The `id` above is shown only to fix the key order; a real append drops it and passes `--id-prefix`, which mints the id.

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--json` | JSON object, `-` or `@<path>` | The new row. Optional when a [field flag](#field-flags) is given. | required |
| `--set` / `--set-json` / `--set-file` | `KEY=…`, repeatable | [Field flags](#field-flags), merged over `--json`. | none |
| `--array` | name | Target array-of-tables, e.g. `rollback_events`. | `items` |
| `--dedupe-by` | `f1,f2,…` | Skip the add when an existing row equals the payload on every listed field, compared as raw strings; the envelope then reports `"added":0` and the `matched_id`. `dedup_id` is never implied — name it for fingerprint dedup. The pre-scan runs before `dedup_id` auto-populates, so a payload's own fingerprint never matches itself. | off |
| `--id-prefix` | prefix | Mint the row's id as `<prefix>` + the next number inside the write lock, so two concurrent adds never mint the same id. The envelope reports it as `"id"`; a payload that already carries an `id` is refused (`kind=validation`). Numbers against the `--array` target. | off |

**Minting the id.** Prefer `--id-prefix` to reading [`items next-id`](#items-next-id) and pasting the result into the payload; with the global `--get id` the call prints just the minted id:

```bash
tomlctl items add ledger.toml --id-prefix R --get id --json '{"summary":"...","status":"open"}'
# R23
```

The envelope without `--get` is `{"ok":true,"added":1,"id":"R23","created":false,"path":"ledger.toml"}`. Under `--dedupe-by`, a skipped add mints nothing and reports the matched row's id as `"id"` (alongside `matched_id`), so `--get id` names the surviving row on both outcomes.

`dedup_id` is auto-populated by the write funnel if the payload doesn't set it — see [Dedup fingerprint contract](#dedup-fingerprint-contract). Rendered output (e.g. PROGRESS-LOG columns) is unaffected; the field only appears in the TOML.

Date-shaped strings (`YYYY-MM-DD`) in the `DATE_KEYS` set — `created`, `updated`, `first_flagged`, `last_updated`, `resolved`, `date`, `promoted`, `dismissed`, `last_seen` — are automatically promoted to TOML date literals. The `DATE_KEYS` constant in `tomlctl/src/convert.rs` owns the set; check it there when a field you expected to promote stayed a quoted string.

### `items add-many`

Appends many rows — e.g. a 50-finding review batch — in one parse, one lock, one rewrite, one sidecar refresh.

Two forms, the same on every platform (see [Stdin input for large JSON payloads](#stdin-input-for-large-json-payloads) for why): a staging file written with the Write tool, which carries any number of rows of any length, and the single-line pipe for a few short machine-built rows.

**Staging file** — write the NDJSON with the `Write` tool, then point `--ndjson` at the path:

```bash
tomlctl items add-many .claude/flows/foo/review-ledger.toml --id-prefix R \
  --defaults-json '{"first_flagged":"2026-04-18","rounds":1,"status":"open"}' \
  --ndjson .claude/flows/foo/_batch.ndjson
# → {"ok":true,"added":N,"ids":["R24",…],"created":false,"path":".claude/flows/foo/review-ledger.toml"}
```

**Pipe, one row per argument** — a single-line command; `printf '%s\n'` emits each argument on its own line, so the rows arrive as NDJSON on stdin:

```bash
printf '%s\n' '{"summary":"..."}' '{"summary":"..."}' | tomlctl items add-many ledger.toml --id-prefix R --ndjson - --defaults-json '{"status":"open"}'
```

The PowerShell spelling pipes a string array; each element becomes one line:

```powershell
'{"summary":"..."}','{"summary":"..."}' | tomlctl items add-many ledger.toml --id-prefix R --ndjson - --defaults-json '{"status":"open"}'
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--ndjson` | `-` or path (a leading `@` is accepted) | One JSON object per line; blank lines are ignored, and a malformed line aborts the whole batch before mutation, naming its line number. | required |
| `--defaults-json` | JSON object, `-` or `@<path>` | Fields stamped on every row; a row's own key wins. Omit for fully-formed rows; `@defaults.json` pairs with `--ndjson -`. | none |
| `--array` | name | As on `items add`. | `items` |
| `--dedupe-by` | `f1,f2,…` | As on `items add`, per row; skipped rows add `"skipped":M` and `"skipped_rows":[{"row":N,"matched_id":"…"}]` to the envelope. | off |
| `--id-prefix` | prefix | As on `items add`, minting one id per added row in input order; the envelope lists them as `"ids"`. A row that carries an `id` refuses the whole batch. | off |

### `items update`

Patches the row whose `id` matches. The patch is a shallow merge; unmentioned fields stay untouched.

```bash
tomlctl items update .claude/flows/foo/review-ledger.toml R22 --json '{"status":"applied","resolved":"2026-04-17","resolution":"Fixed in ab12cd3"}'

# Flip deferred -> open and drop the defer triggers in a single rewrite
tomlctl items update ledger.toml R7 --set status=open --set-json rounds=2 --unset defer_reason --unset defer_trigger
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--json` | JSON object, `-` or `@<path>` | The patch. At least one of `--json`, a field flag or `--unset` is required. | none |
| `--set` / `--set-json` / `--set-file` | `KEY=…`, repeatable | [Field flags](#field-flags), merged over `--json` into one patch. | none |
| `--unset` | key, repeatable | Drop a field. Runs after the `--json` merge, so it wins over a same-key set; a key the row lacks is a no-op. | none |
| `--array` | name | As on `items add`. | `items` |

`dedup_id` is recomputed by the write funnel when the patch touches a fingerprinted field (`file`, `summary`, `severity`, `category`, `symbol`) and does not set `dedup_id` explicitly. See [Dedup fingerprint contract](#dedup-fingerprint-contract).

### `items remove`

Rare — IDs are never renumbered per spec — but occasionally needed for manual cleanup. Fails if the id does not exist.

```bash
tomlctl items remove .claude/flows/foo/review-ledger.toml R17
```

**The id high-water mark.** A removal — by `items remove` or by an `items apply` remove op — records the removed id's number in a root `[id_high_water]` table, keyed by prefix (`R = 17`), whenever it exceeds the stored value. Minting (`--id-prefix`) and `items next-id` then number from `max(highest existing, high-water) + 1`, so removing the top row never lets its id be minted again for a different finding. Do not edit the table by hand.

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--array` | name | As on `items add`. | `items` |

### `items apply`

Runs a mixed add/update/remove batch against one array in a single parse + rewrite. Each op is `{"op": "add|update|remove", ...}` with the single-op payload shape: `json` for add/update (the patch key is `json`, not `set`), `id` for update/remove, on update an optional `unset` array of field names applied after the `json` merge, as `--unset` is, and on update/remove an optional `expect` precondition (below). Ops run in order; any op error aborts the whole batch and the file is left unchanged.

Stage a batch with the Write tool — here `.claude/flows/foo/_ops.ndjson`, one op per line:

```json
{"op":"add","json":{"severity":"minor","summary":"...","status":"open"}}
{"op":"update","id":"R22","json":{"status":"applied","resolved":"2026-04-17"},"unset":["defer_reason"]}
{"op":"remove","id":"R17"}
```

then pass the path, quoted so PowerShell keeps the `@`:

```bash
tomlctl items apply .claude/flows/foo/review-ledger.toml --id-prefix R --ops '@.claude/flows/foo/_ops.ndjson'
# → {"ok":true,"ids":["R24"],"created":false,"path":"...","skipped_stale":[]}

printf '%s\n' '{"op":"update","id":"R22","json":{"status":"applied"}}' '{"op":"remove","id":"R17"}' | tomlctl items apply .claude/flows/foo/review-ledger.toml --ops -
```

**Minting ids for add ops.** With `--id-prefix P`, every add op is minted `P<next>` inside the write lock, in op order, and the envelope lists them as `ids`; the dry-run preview reports the same `ids`. An add op that carries its own `id` is refused (`kind=validation`, naming the op), as `items add --id-prefix` refuses one, so never number add ops by hand. Without `--id-prefix` each add op must carry its `id` and the envelope has no `ids`.

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--ops` | JSON array or NDJSON, `-` or `@<path>` | The batch. A payload whose first non-whitespace character is `{` is read as NDJSON, one op per line: blank lines are skipped and a malformed line aborts the batch naming its 1-based line number. | required |
| `--id-prefix` | prefix | Mint each add op's id under the write lock, in op order, and report them as `"ids"`. No add op may carry an `id`. | off |
| `--array` | name | Run the batch against another array-of-tables, e.g. `rollback_events`. | `items` |
| `--no-remove` | — | Reject any `remove` op. The apply flows pass it so an agent-generated payload cannot erase audit history. | off |
| `--on-stale` | `abort` \| `skip` | What to do with an op whose `expect` no longer matches its row. `abort` fails the batch and writes nothing; `skip` drops the stale ops and applies the rest. | `abort` |

#### Stale-write precondition (`expect`)

An `update` or `remove` op may carry `"expect": {"<field>": <json>, ...}`, checked against the row's current values before the op runs. It is a compare-and-set for a caller that read the ledger earlier and writes later, while a human (glimpse) or another run may have changed the row in between.

- **Comparison.** JSON equality after the TOML→JSON conversion, so a date compares as its `YYYY-MM-DD` string. `null` means the field must be absent. Fields are checked in order and the first mismatch is the one reported.
- **Errors.** `expect` on an `add` op is an error, as is a non-object `expect`. An `expect` naming an id that does not exist passes; the op itself then fails on the unknown id.
- **`--on-stale abort`** (default) runs the whole batch first, then fails naming every stale op — ``op[N] id = R3: `status` expected "open" but found "deferred"`` — and writes nothing.
- **`--on-stale skip`** drops the stale ops, applies the rest, and the envelope always carries a `skipped_stale` array (empty when nothing was stale), on `--dry-run` as well:

```bash
printf '%s\n' '{"op":"update","id":"R22","json":{"status":"wontfix","wontfix_rationale":"..."},"expect":{"status":"open"}}' | tomlctl items apply .claude/flows/foo/review-ledger.toml --ops - --on-stale skip
# → {"ok":true,"created":false,"path":"...","skipped_stale":[{"id":"R22","field":"status","expected":"open","found":"deferred"}]}
```

Report the `skipped_stale` ids as changed during the run and do not retry them; the current value wins.

> **Old-binary hazard.** A tomlctl that predates `expect` ignores unknown op keys, so `expect` alone is silently dropped and the write lands unguarded. Always pass `--on-stale skip` (or `--on-stale abort`) together with `expect`: an older binary rejects the unknown flag loudly instead. That failure surfaces at the write, so a carrier that cannot afford it late in a run checks that `tomlctl capabilities` lists the `items_apply_expect` feature up front.

Prefer this over looping single-op invocations — one parse + one write instead of N. For homogeneous add-only batches prefer `items add-many` (simpler input shape). For append-only non-`items` arrays prefer `array-append`.

`items next-id`, `items find-duplicates`, `items orphans`, `items sweep`, and `items clusters` take no `--array`: they are ledger-schema specific and only reason about `[[items]]`.

### `items next-id`

Prints the next id bare, on one line with no quotes: the prefix + `max(existing numeric suffixes, the [high-water mark](#items-remove)) + 1`. Read-only — it reserves nothing, so an id it prints can be taken by a concurrent write before yours lands.

```bash ignore-guidance-lint
tomlctl items next-id .claude/flows/foo/review-ledger.toml --prefix R        # → R23
tomlctl items next-id .claude/flows/foo/review-ledger.toml --infer-from-file # → R23
```

Never feed its output into a write. Every minting path mints inside the write lock: [`items add --id-prefix`](#items-add), [`items add-many --id-prefix`](#items-add-many), [`items apply --id-prefix`](#items-apply) for the add ops of a mixed batch, and [`flow record`](flow.md) for execution-record entries. `next-id` remains for inspection only.

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--prefix` | letter | The id prefix. On a missing file returns `<prefix>1` as a bootstrapping fast path; [strict reads](query.md#strict-reads---strict-read) disable that. Required unless `--infer-from-file` is given. | — |
| `--infer-from-file` | — | Take the prefix from the existing ids. Cannot bootstrap: errors on an empty ledger (`--infer-from-file requires a non-empty ledger or explicit --prefix`) and on more than one prefix (`--infer-from-file found multiple prefixes (R, O); pass --prefix explicitly`). Excludes `--prefix`. | off |

### `array-append`

Appends records to an arbitrary array-of-tables such as `[[rollback_events]]` (written by the `/review-apply` / `/optimise-apply` rollback protocol). A thin shim over `items add-many` that takes the array name positionally and needs no op framing. The envelope reports `appended`.

```bash
tomlctl array-append <ledger> rollback_events --set timestamp=2026-04-18T14:32:00Z --set command=review-apply --set cause="build failure" --set-json 'items=["R3","R7"]' --set stash_ref=3f9c2a7e5b1d4c8e9a0f6b2d7c3e1a5f8b4d9c02
tomlctl array-append <ledger> rollback_events --ndjson .claude/flows/foo/_rollback-batch.ndjson
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--json` | JSON object, `-` or `@<path>` | One record; the field flags merge over it. | — |
| `--set` / `--set-json` / `--set-file` | `KEY=…`, repeatable | [Field flags](#field-flags) building one record, alone or over `--json`. | — |
| `--ndjson` | `-` or path (a leading `@` is accepted) | Many records, one per line — the same staging-file rule as `items add-many`. Excludes `--json` and the field flags. | — |

One source is required: `--json`, a field flag, or `--ndjson`.

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

All JSON-accepting flags (`--ops`, `--json` on `items add` / `items update` / `set-json` / `array-append`, `--defaults-json` / `--ndjson` on `items add-many` / `array-append`) take three spellings: a literal, `-` for stdin, or `@<path>` to read a file. Stdin is capped at 32 MiB and a payload past the cap is an error, never a silent partial batch; tomlctl refuses to block on an interactive TTY; and only one flag per invocation may be `-`, because a process has one stdin (a second errors with `stdin already consumed by another flag on this invocation`). Quote an `@<path>` value (`'@ops.json'`): PowerShell drops an unquoted `@name` as splatting syntax.

Pick the input by what you are writing, the same way on every platform:

- **One entry** — the [field flags](#field-flags). Write any prose to a file with the Write tool and pass it with `--set-file`; short values go in `--set` / `--set-json`. An execution-record entry goes through [`flow record`](flow.md), which takes the same flags.
- **A verb without field flags** (`backlog add` / `check`, `json set`, `inputs`, `tasks add-many`) — inline single-line JSON for a short machine-built value; otherwise stage the payload with the Write tool and pass `--json '@<path>'` or `--ndjson <path>`. A verb that reads a text value from `-` takes a staged file by redirection (`backlog check --summary - < .claude/flows/<slug>/_summary.txt`).
- **Many entries or a whole payload** — stage it with the Write tool and pass `--ndjson <path>` or `--json '@<path>'` / `--ops '@<path>'`. A staged file has no size limit and no quoting to get wrong.
- **A few short machine-built rows** — the single-line pipe, `printf '%s\n' '<row>' '<row>' | tomlctl … --ndjson -` (or a PowerShell string array); see [`items add-many`](#items-add-many) for both spellings. It is one command line, not a heredoc.

```bash
# Write tool → .claude/flows/<slug>/_batch.ndjson  (one JSON object per line), then:
tomlctl items add-many .claude/flows/<slug>/review-ledger.toml --id-prefix R \
  --defaults-json '{"first_flagged":"2026-04-24","rounds":1,"status":"open"}' \
  --ndjson .claude/flows/<slug>/_batch.ndjson
```

**No multi-line heredocs.** On Windows they fail intermittently: the Bash-tool transport to Git Bash can append CR bytes to the terminator, so the body either aborts the whole command (`bash: -c: line N: unexpected EOF while looking for matching \`''`) or lands partly — tomlctl writes the first rows, then bash runs the rest of the body as shell commands (`/c/Users/ros…: Permission denied`), which surfaces as a "false interrupt". A PowerShell here-string or long string-array pipe fails the same way. A staged file avoids every one of these, so it is the form on Linux and macOS too.

When `-` meets an interactive terminal instead of a pipe, tomlctl refuses rather than waiting; the error names the three ways out — stage the text in a file and redirect it in, use `--set-file` where the verb has field flags, or pass the value literally.

## Dry-run

`--dry-run` is accepted on every write subcommand — `set`, `set-json`, `array-append`,
`items add`, `items add-many`, `items update`, `items remove`, `items apply`,
`items backfill-dedup-id`, and `items sweep --update` (there it requires `--update`, since
the read-only sweep has nothing to preview). It reports the computed mutation and touches no
file or sidecar. The dry-run path runs the same compute stage as the real path — mutation
logic cannot drift between preview and apply. A target outside `.claude/` still needs
`--allow-outside`.

The envelope takes three shapes. `set` and `set-json` report `kind: "scalar"` with the old and
new value at the path; a `set` with more than one pair reports a `pairs` array of those
objects in place of `would_change`. Every `items` verb, `array-append` and `items sweep --update` report
`kind: "items"` with per-op counts and `ids`, the union of every affected id — a row with
no `id`, such as an `array-append` record, is counted but not listed. `items backfill-dedup-id`
reports `would_backfill` and the ids it would stamp. Under `--id-prefix`, the preview's `ids`
are the ids the real write would mint.

```bash
tomlctl set foo.toml status review --dry-run
# {"ok":true,"dry_run":true,"would_change":{"kind":"scalar","path":"status","old":"draft","new":"review"}}

tomlctl set .claude/flows/<slug>/context.toml status in-progress --set tasks.total=17 --dry-run
# {"ok":true,"dry_run":true,"pairs":[{"kind":"scalar","path":"status","old":"draft","new":"in-progress"},{"kind":"scalar","path":"tasks.total","old":0,"new":17}]}

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
