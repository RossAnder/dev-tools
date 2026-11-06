# tomlctl — flow, integrity and maintenance reference

The cross-cutting half of the tomlctl surface: what `--verify-integrity` is accepted on and
what the `.sha256` sidecar does and does not promise, the machine-readable error envelope,
`flow record` (the one write path into a flow's execution record), the two `flow` verbs that
emit rather than mutate flow state (`render-progress-log` and `envelope build`), the envelopes
`flow init` and `flow list` return, how the registry stores `binding.worktree`, what
`flow doctor --fix` repairs, and the infrastructure-only `blocks` verbs. The read verbs live in
[query.md](query.md) and the mutating ones in [write.md](write.md); the flow registry's own
verbs (`flow active|init|list|resolve|stale|doctor|find-plans|ensure-artifact`) are listed in
the Quick Reference table of [../SKILL.md](../SKILL.md).

## Contents

- [`--verify-integrity` support matrix](#--verify-integrity-support-matrix)
- [Sidecar files](#sidecar-files)
  - [Line endings](#line-endings)
- [Error format (`--error-format json`)](#error-format---error-format-json)
- [Flow verbs](#flow-verbs)
  - [`flow init`](#flow-init)
  - [`flow active add`](#flow-active-add)
  - [`flow doctor --fix`](#flow-doctor---fix)
  - [`flow list`](#flow-list)
  - [`flow record`](#flow-record)
  - [`flow render-progress-log`](#flow-render-progress-log)
  - [`flow envelope build`](#flow-envelope-build)
- [Advanced / maintenance](#advanced--maintenance)
  - [`blocks verify`](#blocks-verify)

## `--verify-integrity` support matrix

`--verify-integrity` is a **per-subcommand flag**, not a global. A read verb that takes it verifies the target before reading. Every write verb carries it in the write bundle: when the target exists, the verb verifies it inside the write lock, before the read that precedes its write, and a mismatch or a missing sidecar is `kind=integrity` with nothing written. A target not created yet has nothing to verify. The table lists the read verbs and the `plan` writers, whose notes name the stores they check, since a `plan` verb takes a slug rather than a file. Any verb neither listed nor in the write bundle rejects the flag at the clap layer. See [Sidecar files](#sidecar-files) for what it checks.

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
| `tomlctl items sweep` | yes |
| `tomlctl items clusters` | yes |
| `tomlctl sweep` | n/a |
| `tomlctl tasks show` | yes |
| `tomlctl tasks list` | yes |
| `tomlctl tasks edges` | yes |
| `tomlctl tasks ready` | yes |
| `tomlctl tasks batches` | yes |
| `tomlctl tasks train` | yes |
| `tomlctl tasks closure` | yes |
| `tomlctl tasks check` | yes |
| `tomlctl tasks render` | yes |
| `tomlctl tasks snapshot` | yes — checks `tasks.toml` and each of `execution-record.toml`, `agents.toml`, `context.toml` that is present |
| `tomlctl plan check` | yes — checks `plan.toml` and `tasks.toml` |
| `tomlctl plan view` | yes — checks `plan.toml` and `tasks.toml` when present |
| `tomlctl plan stage` | yes — checks `plan.toml` and `tasks.toml` |
| `tomlctl plan gate` | yes — checks `plan.toml` and `tasks.toml` |
| `tomlctl plan drift` | yes — checks `plan.toml` and `tasks.toml` |
| `tomlctl plan open` | yes — checks an existing `plan.toml`, and `backlog.toml` under `--from` |
| `tomlctl plan decide` | yes — checks `plan.toml` |
| `tomlctl plan contract` | yes — checks `plan.toml` |
| `tomlctl plan handover add` | yes — checks `plan.toml` |
| `tomlctl plan handover done` | yes — checks `plan.toml` |
| `tomlctl plan run start` | yes — checks `plan.toml` |
| `tomlctl plan run finish` | yes — checks `plan.toml` |
| `tomlctl plan run abandon` | yes — checks `plan.toml` |
| `tomlctl plan run skip` | yes — checks `plan.toml` |
| `tomlctl plan milestone add` | yes — checks `tasks.toml` |
| `tomlctl plan milestone update` | yes — checks `tasks.toml` |
| `tomlctl plan probes` | yes — checks `tasks.toml`, under `--emit` and `--record` alike |
| `tomlctl plan approve` | yes — checks `plan.toml` and `tasks.toml` first, then each store it writes |
| `tomlctl agents list` | yes |
| `tomlctl backlog check` | yes |
| `tomlctl backlog list` | yes |
| `tomlctl backlog show` | yes |
| `tomlctl backlog get` | yes |
| `tomlctl backlog next` | yes |
| `tomlctl backlog cluster` | yes |
| `tomlctl backlog evidence dir` | yes |
| `tomlctl backlog evidence audit` | yes |
| `tomlctl inputs list` | yes |
| `tomlctl review list` | yes |
| `tomlctl review show` | yes |
| `tomlctl review digest` | yes |
| `tomlctl review report` | yes |
| `tomlctl review stats` | yes |
| `tomlctl review catalogue` | yes |
| `tomlctl review schema` | yes |
| `tomlctl flow doctor` | accepted without `--fix`, where its sidecar checks run regardless; refused with `--fix`, which repairs what the flag refuses |
| `tomlctl flow list` | yes |
| `tomlctl flow render-progress-log` | yes — checks `execution-record.toml` only |

`tomlctl blocks verify` intentionally does NOT accept `--verify-integrity` (it operates on markdown with no sidecar pair), and neither does the standalone `tomlctl sweep` — it searches tracked files and reads no TOML at all. `tomlctl items sweep` accepts the flag through its write bundle (it can `--update` the ledger), so it takes no `--strict-read`; `tomlctl items clusters` is a plain read verb and takes both.

## Sidecar files

Every write produces (and every read can verify) two sidecars, one beside the target TOML and one under `.claude/.locks/`:

- **`<file>.sha256`** — integrity sidecar in standard `sha256sum` format (`<64-lower-hex>  <basename>\n`, two spaces between digest and basename, trailing newline). Written by default on every write (atomic tempfile+rename, under the same lock as the primary write). Verified by `--verify-integrity` on reads (see the [support matrix](#--verify-integrity-support-matrix)). Regenerated by `tomlctl integrity refresh <path>`.
- **`.claude/.locks/<hash>.lock`** — exclusive advisory lock acquired by every write path; prevents concurrent mutators from corrupting the file. On Windows this is a mandatory lock; see the lock-recovery note under [Constraints and gotchas](../SKILL.md#constraints-and-gotchas).

```bash
# Default — writes both ledger.toml and ledger.toml.sha256
tomlctl items update ledger.toml R7 --json '{"status":"fixed"}'

# Skip the sidecar (e.g. read-only-ish FS, or hand-editing before the next write).
tomlctl items update ledger.toml R7 --json '{"status":"fixed"}' --no-write-integrity

# Fail on a sidecar refresh failure after the JSON write, rather than warn.
tomlctl json set .claude/config.json key --json '"value"' --strict-integrity

# Verify on read — errors if sidecar is missing OR the digest disagrees.
tomlctl items list ledger.toml --where status=open --verify-integrity
```

- **Missing sidecar under `--verify-integrity`** → hard error naming the expected path; never auto-regenerated. Run `tomlctl integrity refresh` to materialise it, unless the file holds CRLF, which it refuses: see [Line endings](#line-endings).
- **Digest mismatch** → hard error naming both expected (from sidecar) and actual (from current bytes). The cause is an out-of-band edit, a corrupted sidecar, or line endings; when the CRLF-folded bytes match the sidecar, the error adds a clause saying only the line endings differ, and [Line endings](#line-endings) applies. Resolve by a human; `tomlctl` never auto-repairs.
- **Sidecar write failure on a TOML write** (`set`, `set-json`, and the `items`, `tasks`, `backlog` and `flow` writers) → always a hard error. The sidecar is persisted first, so its failure leaves the TOML untouched; a TOML persist that fails after it puts the previous sidecar back, or removes it if there was none, and errors too. `--strict-integrity` has no effect here.
- **Sidecar refresh failure after a successful primary write** — `json set`, `json unset` and `flow ensure-artifact --bootstrap`, which write the primary first → stderr warning and exit 0 by default (the data is durable; the next write rebuilds the sidecar). `--strict-integrity` flips this to a hard error.
- **`--allow-outside`** applies identically — the sidecar lands next to the target wherever that is.
- **`flow doctor`** checks the sidecars of `context.toml`, the execution record and `tasks.toml`, plus `plan.toml`'s through a `plan-sidecar` check that appears only when that file exists.

> `.sha256` is not a MAC — it detects accidental corruption and out-of-band edits, not an adversary with write access. Hostile-actor threat models still require auditing the ledger's git history.

### Line endings

Digests cover a file's raw bytes. A checkout that rewrites a store to CRLF therefore fails its sidecar over a change nobody made, and a digest stamped over those CRLF bytes would fail on every LF checkout of the same commit. So `integrity refresh`, `flow doctor --fix`, and `flow init` / `flow ensure-artifact` over a store its sidecar does not cover all refuse a `.toml` holding CRLF, naming the fix below.

If the store shows CRLF (`git ls-files --eol <store>` reports `w/crlf`), recover in this order, and do not run `integrity refresh` or `flow doctor --fix` first:

1. Pin `.claude/**/*.toml` and `.claude/**/*.sha256` to `text eol=lf` in that repo's `.gitattributes`.
2. Remove the store and its `.sha256` sidecar. Git treats a CRLF working copy of a file whose index form is LF as unchanged, so a checkout alone leaves it CRLF.
3. `git checkout -- <store> <store>.sha256`. This restores the index form, so a write to the store not yet staged is lost.

## Error format (`--error-format json`)

`--error-format json` is a global flag on the top-level command. When set, errors are written to **stderr** as a compact single-line JSON envelope:

```
{"error":{"kind":"<kind>","message":"<prose>","file":null|"<path>","arg":null|"<arg>"}}
```

`arg` names the CLI argument the error is about by its clap id (e.g. `to`, `ids`) where a verb tags one — `backlog triage` does — and is `null` otherwise. Exit code stays 1 on error. Success paths are unchanged — text output on success is byte-identical to default mode.

```bash ignore-guidance-lint
tomlctl --error-format json items list /nonexistent/ledger.toml 2>&1 >/dev/null
# {"error":{"kind":"not_found","message":"...","file":"/nonexistent/ledger.toml","arg":null}}
```

Closed taxonomy (the table lists representative sites per kind, not every call site; untagged `bail!` sites fall through to `other`):

| `kind` | Emitted from |
|---|---|
| `not_found` | `io.rs` — target file missing at the path the caller passed; `flow record` — no `context.toml` for `--slug`; `backlog triage` — no item with the given id |
| `integrity` | `integrity.rs` — sidecar hash mismatch or missing under `--verify-integrity` |
| `parse` | `io.rs` — malformed TOML at the document root |
| `validation` | `query.rs` / `items.rs` — flag-mutex violations, `items next-id` prefix shape rejections, `--infer-from-file` empty/multi-prefix errors; `flow record` — an entry the execution-record contract refuses; `backlog check --ndjson` — a per-line validation error; `backlog check --summary '@<file>'` — a summary payload the validation refuses |
| `other` | any untagged error — the downcast returned `None` |

Prefer `--error-format json` + `.error.kind` switching over regex-matching stderr text when branching on error class (e.g. "bootstrap the ledger if missing, bubble up otherwise").

## Flow verbs

`flow init`, `flow active add`, `flow doctor` and `flow record` carry the shared write bundle — `--allow-outside`, `--no-create`,
`--no-write-integrity`, `--strict-integrity`, `--verify-integrity`, plus `--no-stamp` on
`flow record` — and `flow list` and
`flow render-progress-log` the read bundle, `--verify-integrity` and `--strict-read`. A table
below lists a bundle flag only where the verb gives it its own meaning. `flow envelope build`
takes neither bundle. Every verb also takes the global `--error-format`.

### `flow init`

Seeds `context.toml`, `execution-record.toml`, `tasks.toml` and the active-flow registry entry in one re-runnable call. Because it is idempotent, the envelope reports the `context.toml` verdict and the set of stores actually written as two separate keys:

```bash
tomlctl flow init --slug <slug> --plan docs/plans/<slug>.md
# → {"ok":true,"slug":"<slug>","action":"init",
#    "created":[".claude/flows/<slug>/context.toml",".claude/flows/<slug>/execution-record.toml",".claude/flows/<slug>/tasks.toml"],
#    "context_path":".claude/flows/<slug>/context.toml","artifacts":{...}}
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` | slug | Flow slug, matching `^[a-z0-9][a-z0-9-]{0,63}$`. | required |
| `--plan` | path | Plan markdown the flow tracks, recorded as `plan_path` repo-relative with forward slashes: an absolute path under the repo root is relativised, and a relative one resolves against the working directory when it names a file there, the repo root otherwise. A path outside the root, one escaping it with `..`, or one not ending `.md` is refused with `kind=validation` before anything is written, `--dry-run` included. | required |
| `--branch` | name | `branch` recorded in `context.toml` and on the active-flow entry. | omitted |
| `--worktree` | path | `worktree` recorded on the active-flow entry, [relative to the main worktree](#flow-active-add). | omitted |
| `--scope` | glob | Scope glob recorded in `context.toml` and on the active-flow entry. Repeatable. | `[]` |
| `--dry-run` | — | Write nothing; emit `{"ok":true,"dry_run":true,"would_change":{…}}` in place of the envelope. | off |

- **`action`** — `"init"` when this run wrote `context.toml`, `"noop"` when it was already present. It describes `context.toml` alone.
- **`created`** — the repo-relative stores this run materialised, in creation order; `[]` when every store was already there. Sidecars are excluded: refreshing a `.sha256` beside an existing store is a repair, not a creation.

Branch on `created`, not on `action`, to detect a freshly-seeded store. `flow init` is the only sanctioned route that seeds `tasks.toml` — `flow ensure-artifact --bootstrap --kind tasks` is deliberately a no-op — so a legacy flow carrying only `context.toml` returns `action: "noop"` with `created` naming the `execution-record.toml` and `tasks.toml` it just minted.

### `flow active add`

Adds a row to `.claude/active-flow.toml`, the tracked registry of active flows, or replaces the row that slug already has. The `[active.binding]` fields are advisory hints for `flow resolve`, not part of the row's identity:

```toml
schema_version = 1
[[active]]
slug = "feature-x"
last_used = "2026-05-08T14:32:00Z"
[active.binding]
branch = "feat/x"
worktree = "."
scope = ["src/foo/**"]
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` | slug | The row's key. | required |
| `--branch` | name | `binding.branch`. | omitted |
| `--worktree` | path | `binding.worktree`, stored as described below. | omitted |
| `--scope` | glob | `binding.scope`. Repeatable. | `[]` |
| `--dry-run` | — | Write nothing; emit `{"ok":true,"dry_run":true,"would_change":{…}}`. | off |

Every clone and linked worktree holds a copy of this file, so `binding.worktree` is stored relative to the main worktree's root, with `/` separators: `.` for the primary checkout, `sub` for a path under it, `../x` for a sibling linked worktree. The main worktree is the parent of the git common dir. The repo root stands in for it when git cannot answer, when that root is not a git top level (`TOMLCTL_ROOT` inside a repo), and in a submodule or bare repo.

- **Writes.** `flow active add` and `flow init` turn an absolute `--worktree` into that spelling. An absolute path with no relative spelling, on another drive or in another platform's form, is stored as given. A relative `--worktree` is read as already relative to the main worktree, not to the working directory, so `--worktree .` in a linked worktree names the primary checkout.
- **Reads.** `flow resolve --worktree` accepts relative and legacy absolute values, on the flag and in the file alike, and compares the paths they resolve to. An absolute value from another machine neither errors nor matches.
- **Normalising.** `flow active touch` (for its own row) and [`flow doctor --fix`](#flow-doctor---fix) rewrite an absolute value at or under the main worktree to its relative spelling, and leave every other value as it is.

### `flow doctor --fix`

Applies the repairs below and lists each in the envelope's `fixes_applied`, by `name`, with its `scope`, `action` and `ok`. With `--dry-run` it writes nothing and reports what it would do. It never creates a missing artifact, and `--verify-integrity` is refused with `kind=validation`. A row still holding a machine-local worktree after the pass, or on a run without `--fix`, raises one aggregated line on `warnings`.

| Fix `name` | Repairs | Leaves alone |
|---|---|---|
| `sidecar-refresh` | A missing or mismatched `.sha256` sidecar, regenerated from the file's current bytes. | A `.toml` holding CRLF, refused with `ok: false`; see [Line endings](#line-endings). |
| `active-prune` | A registry row whose flow directory no longer exists, removed. | Rows whose flow exists. |
| `active-worktree-relative` | A surviving row's absolute `binding.worktree` at or under the main worktree, rewritten to its relative spelling. | Relative values, absolute values outside the main worktree (a sibling path or another machine's), and values in another platform's form. |
| `artifacts-tasks-backfill` | An absent `[artifacts].tasks` key in `context.toml`, inserted with its canonical value. | A key already present, canonical or not. |

### `flow list`

Reads every `.claude/flows/<slug>/context.toml` and returns the readable ones under `flows` and the unreadable ones under `skipped`:

```bash
tomlctl flow list --status draft
# → {"ok":true,
#    "flows":[{"slug":"<slug>","status":"draft","updated":"<date>","plan_path":"docs/plans/<slug>.md","branch":"<branch>","scope":[]}],
#    "skipped":[{"path":".claude/flows/<other>/context.toml","reason":"TOML parse error at line 2: key with no value, expected `=`"}]}
```

- **`flows`** — one row per readable flow that passes the filters. `status`, `updated` and `plan_path` are `""` when absent; `branch` is omitted when absent; `scope` defaults to `[]`.
- **`skipped`** — one entry per `context.toml` that could not be read or parsed, with its repo-relative `path` and a one-line `reason`. `--status`, `--branch` and `--active-only` never hide an entry here, since an unreadable file has no fields to filter on. `ok` stays `true` when rows are skipped.

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--status` | status | Keep flows whose `context.toml` `status` matches exactly. | all |
| `--branch` | name | Keep flows whose `context.toml` `branch` matches exactly. | all |
| `--active-only` | — | Keep only slugs present in `.claude/active-flow.toml`. | off |
| `--verify-integrity` | — | Also skip a `context.toml` whose `.sha256` sidecar is missing or does not match, with the verifier's message as the `reason`. | off |
| `--strict-read` | — | Turn the first skipped `context.toml` into an error instead — `kind=integrity` for a sidecar failure, `kind=parse` otherwise. | off |

### `flow record`

Appends one validated entry, or an all-or-nothing batch, to a flow's execution record: the file
`context.toml` `[artifacts].execution_record` names, else
`.claude/flows/<slug>/execution-record.toml`. A custom `execution_record` path that stays under the
repo root is supported — `flow doctor` reports it as a warning, not a failure — but `tasks snapshot`
reads only the sibling `execution-record.toml`. It is the only write path for that file. What each
entry type must carry is the `flow-contract-execution-record-schema` skill's; this is the flag
table and the checks the verb enforces.

```bash
tomlctl flow record --slug <slug> --type task-completion --task 7 --set agent=implement --set status=done --set dispatch_tier=deep --set dispatch_agent=implement-deep --set vet=skipped --set retries=0 --set-json 'files=["src/parser.rs"]' --set-file summary=<staged-file>
# → {"ok":true,"id":"E12","type":"task-completion","task_ref":"add-the-parser","truncated":[],"dropped_files":[],"scope_warnings":[],"path":".claude/flows/<slug>/execution-record.toml"}
tomlctl flow record --slug <slug> --type verification --set agent=implement --set 'summary=checkpoint B build and tests' --set 'command=cargo nextest run' --set outcome=pass --get id
# Several entries at once — stage the NDJSON with the Write tool; the flags are per-row defaults:
tomlctl flow record --slug <slug> --set agent=implement --ndjson <staged-file> --get id
# N tasks' records in one call — each row names its task with `task`; a TOML record file is the form for prose:
tomlctl flow record --slug <slug> --set agent=implement --ndjson <staged-record.toml> --get id
```

A TOML record file holds one `[[record]]` table per entry, with `'''` strings for prose:

```toml
[[record]]
type = "task-completion"
task = 7
status = "done"
summary = '''Added the retry guard.'''

[[record]]
type = "task-completion"
task = 9
status = "done"
summary = '''Moved the token reader.'''
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` | slug | Flow whose record is written. A slug with no `context.toml` is `kind=not_found`. | required |
| `--type` | type | `task-completion`, `verification`, `deviation`, `deferral`, `reconcile`, `status-transition` or `checkpoint`. Required unless every `--ndjson` row carries its own. | — |
| `--task` | id | Task id in the flow's `tasks.toml`; that row's `ref` becomes `task_ref`. A payload `task_ref` that disagrees is refused with `kind=validation`. | none |
| `--json` | object, `@<path>` or `-` | Base payload; the field flags are laid over it and win. | none |
| `--set` | `KEY=VALUE`, repeatable | A string field; a dotted KEY nests. A digit string for `retries`, `duration_s` or `findings_count` is stored as an integer. | none |
| `--set-json` | `KEY=JSON`, repeatable | Any JSON value — the form for the array fields `files`, `commits`, `failed_ids` and `commits_checked`. | none |
| `--set-file` | `KEY=PATH`, repeatable | The text of a file (`-` reads stdin), less one leading BOM and one trailing newline — the form for prose. | none |
| `--ndjson` | path, `@<path>` or `-` | One entry per line, written all or nothing. `--json` and the field flags are defaults a row's own keys override; `--type` and `--task` apply to every row, and a row whose own `type` or `task_ref` disagrees is refused with `kind=validation`. A row may carry `task` (a task id): it is resolved per row to `task_ref` and not stored, and is refused when it disagrees with `--task` or with the row's own `task_ref`. A `.toml` path is read as a record file of `[[record]]` rows. | none |
| `--dry-run` | — | Validate and report the ids that would be minted; write nothing. | off |

A key given twice across the field flags is `kind=validation`. Under Git Bash a `--set` value
starting with `/` is rewritten into a Windows path, so pass such a value through `--set-file`.

What the verb does to every entry before the write:

- **Fills** `date` with today (UTC) when absent, and mints the id as `E<n>` under the write lock.
- **Requires** `type`, `date`, `agent` and `summary`, plus the type's own fields; a missing one is
  `kind=validation` naming it. `commits` is optional on `task-completion` and `deviation`.
- **Checks enums** on `task-completion` (`status`, `dispatch_tier`, `dispatch_agent`, `vet`),
  `verification` (`outcome`), `reconcile` (`direction`) and `status-transition` (`from_status`
  and `to_status`, each one of `draft`, `in-progress`, `review`, `complete`). A `checkpoint`
  entry is free-form beyond the required four, and keys the contract does not name pass through.
- **Checks types**: `retries`, `duration_s` and `findings_count` must be non-negative integers,
  and a digit string there is stored as the integer; `files`, `commits`, `failed_ids` and
  `commits_checked` must be arrays, and a string there is refused with a hint naming
  `--set-json`.
- **Caps text** rather than refusing it: `summary` at 1024 bytes and `description`, `rationale`,
  `original_intent`, `reason` and `reevaluate_when` at 8192, cut at a character boundary and
  ending ` (truncated)`; `failed_ids` keeps its first 20. Each cut field is named in `truncated`.
- **Cleans `files`**: `\` becomes `/`, and an absolute, `~`-relative, drive-letter or `..` entry
  is dropped and named in `dropped_files`. A non-empty list that dropping would empty is refused.
  A kept entry outside the `context.toml` `scope` globs is named in `scope_warnings` and sets
  `scope_warning = true` on the entry; an empty or absent scope warns on nothing.

A single entry prints the object shown above, with `"dry_run":true` added under `--dry-run`. A
batch prints one envelope whose `rows` are the per-entry reports, so `--get id` lists the minted
ids one per line:

```json
{"ok":true,"ids":["E12","E13"],"path":".claude/flows/<slug>/execution-record.toml",
 "rows":[{"id":"E12","type":"verification","task_ref":null,"truncated":[],"dropped_files":[],"scope_warnings":[]},
         {"id":"E13","type":"deviation","task_ref":"add-the-parser","truncated":[],"dropped_files":[],"scope_warnings":[]}]}
```

Re-render `PROGRESS-LOG.md` with `flow render-progress-log` after recording.

### `flow render-progress-log`

Regenerates `.claude/flows/<slug>/PROGRESS-LOG.md` deterministically. The output is a pure function of two inputs: the flow's execution record (the file `context.toml` `[artifacts].execution_record` names, else `execution-record.toml` in the flow dir) and the flow title (read from `context.toml`'s `plan_path` → the plan file's `# Plan:` header). Call this verb rather than re-deriving the tables by hand.

```bash
tomlctl flow render-progress-log --slug <slug>
# → {"ok":true,"path":".claude/flows/<slug>/PROGRESS-LOG.md","tables":{"completed":N,"deviations":N,"deferrals":N,"sessions":N}}
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` | slug | Flow whose `PROGRESS-LOG.md` is regenerated. | required |
| `--stdout` | — | Print only the rendered Markdown to stdout, write no file, and emit no JSON envelope. | off |
| `--verify-integrity` | — | Check `execution-record.toml` against its sidecar before rendering. | off |

The rendered document carries its marker line followed by four tables — **Completed Items**, **Deviations**, **Deferrals**, and **Session Log** — each with a `(none)` empty-state row when it has no entries, plus a trailing newline. The envelope's `tables` counts mirror the row counts of each table.

PROGRESS-LOG.md is a **derived artifact**: because it is fully regenerable from `execution-record.toml`, the renderer writes **no `.sha256` sidecar** for it (unlike the source TOML files). There is nothing to integrity-check on the rendered output — re-run the verb to reproduce it.

### `flow envelope build`

Emits the canonical `flow-bootstrap` input envelope as JSON on stdout — what every flow command's Step-0 dispatch sends. Pure / read-only — no filesystem writes, no flow-state mutation. The emitted shape matches the `Contract` section of `claude/agents/flow-bootstrap.md` byte-for-byte; bumping that contract means bumping this subcommand in lock-step.

```bash
tomlctl flow envelope build \
  --command review \
  --branch "$(git branch --show-current)" \
  --worktree "$(git rev-parse --show-toplevel)" \
  --cwd "$(pwd)"
# {"command":"review","flow_override":null,"path_args":[],"branch":"main","worktree":"/abs/work","cwd":"/abs/cwd","require_artifacts":[],"staleness_threshold":"7d"}
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--command` | carrier | One of `review`, `optimise`, `plan-new`, `plan-update`, `implement`, `review-plan`, `tdd`, `review-apply`, `optimise-apply`, `test-bootstrap`; anything else is refused with `kind=validation`. | required |
| `--flow-override` | slug | Explicit flow slug, passed through to the bootstrap agent. | `null` |
| `--path-arg` | path | Appended verbatim to `path_args`. Repeatable. | `[]` |
| `--branch` | name | Current git branch, typically `$(git branch --show-current)`; omit on a detached HEAD. | `null` |
| `--worktree` | path | Git worktree top level, typically `$(git rev-parse --show-toplevel)`. | `null` |
| `--cwd` | path | Current working directory, typically `$(pwd)`. | `null` |
| `--require-artifact` | key | Artifact that must exist: `review_ledger`, `optimise_findings`, `execution_record`, `plan_review_findings` or `tasks`; a typo errors with `kind=validation`. Repeatable. | `[]` |
| `--staleness-threshold` | duration | Staleness gate for the bootstrap agent (`"1d"`, `"48h"`, …). | `"7d"` |

An omitted optional flag still appears in the envelope, as `null` or `[]` — the bootstrap agent's contract expects every documented key to be present.

## Advanced / maintenance

Infrastructure-only primitives — no flow command invokes these directly.
`scripts/verify-shared-blocks.sh` is **canonical**: it is what the pre-commit hook runs, and
these are a Rust mirror held no laxer than it, so a tree the shell verifier rejects must never
verify here. Reach for them for an ad-hoc parity check without the bash+gawk dependency. The
in-crate `blocks_verify_reproduces_shell_hashes` and `blocks_verify_agrees_with_shell_gate`
tests pin the mirror to the shell verifier's hashes and to its verdicts.

### `blocks verify`

Checks that named shared blocks are byte-identical across a set of files, mirroring `scripts/verify-shared-blocks.sh` without the bash+awk dependency. Blocks are delimited by `<!-- SHARED-BLOCK:<name> START -->` … `<!-- SHARED-BLOCK:<name> END -->` markers (markers excluded from the hash; content between them joined by `\n`).

```bash
tomlctl blocks verify claude/agents/implement-deep.md claude/agents/implement-lite.md --block forbidden-working-tree-ops --block backlog-candidates
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | files | Markdown files whose spans are compared. | — |
| `--block` | name | Block to verify. Repeatable. Name only blocks the listed files actually carry — a `--block` naming an absent marker exits non-zero; `scripts/shared-blocks.toml` enumerates which files carry which markers. | every block present in the first listed file |

Output is JSON (`{"ok":true|false,"blocks":[...]}`); exit code 0 on success, non-zero on drift, on a missing marker, and on a span that extracts empty between its markers — two carriers whose spans are both empty hash the same digest of nothing, so parity there reports having compared nothing.

A block that lost a file to any of those three carries an optional `defects` array beside its `missing` list, one entry per file with no usable span:

```json
{"name":"backlog-candidates","defects":[{"file":"claude/agents/implement-lite.md","reason":"extracted-empty"}],
 "ok":false,"hash":"<64-lower-hex>","files":["claude/agents/implement-deep.md"],
 "missing":["claude/agents/implement-lite.md"]}
```

`reason` is `missing-marker`, `marker-trailing-cr` (a marker still has a
trailing `\r` after CRLF normalisation) or `extracted-empty`. Each block result
has an `ok` boolean: it is true only when every listed carrier has a non-empty
span and all surviving hashes agree. The key is omitted from neither success nor
failure results, so callers can inspect one block without inferring its status
from the top-level `ok`. Does NOT accept `--verify-integrity` / `--allow-outside` / `--no-write-integrity` / `--strict-integrity` (markdown has no sidecar pair; `blocks verify` never writes).
