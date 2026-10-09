# tomlctl

Small TOML read/write CLI for Claude Code flow and ledger files.

Built because `python3 -c "import tomllib"` is unreliable on Windows Git Bash, and the canonical flow/ledger schemas (`.claude/flows/*/context.toml`, `review-ledger.toml`, `optimise-findings.toml`) require parse-rewrite operations rather than line-level edits.

## Install

```bash
cargo install --path .
```

Requires Rust 1.85+.

## Usage

See [`claude/skills/tomlctl/SKILL.md`](../claude/skills/tomlctl/SKILL.md) for the full reference.

Quick tour:

```bash
tomlctl get         <file> [path]                     # JSON of value (or whole file)
tomlctl set         <file> <path> <value> [--type T]  # scalar
tomlctl set         <file> --set <path>=<value> [--set <path>=<value>]...   # several scalars in one write; types inferred
tomlctl set-json    <file> <path> --json <json>       # array / object / scalar
tomlctl json get    <file> <path>                     # read a value via the json subcommand
tomlctl json set    <file> <path> --json <value>      # write a value (array / object / scalar)
tomlctl json unset  <file> <path>                     # delete a key at path
tomlctl validate    <file>                            # parse-check
tomlctl items list  <file> [--status X] [--category Y] [--newer-than YYYY-MM-DD] [--file PATH] [--count]
tomlctl items get   <file> <id>
tomlctl items add   <file> --json '{"id":"R7",...}' [--id-prefix P]   # --id-prefix mints the id under the lock (payload must not carry one)
tomlctl items add   <file> --id-prefix P --set k=v --set-json k=<json> --set-file k=<path>   # field flags build the payload; they also merge over --json on items update and array-append
tomlctl items add-many <file> --defaults-json '{...}' --ndjson - [--id-prefix P]    # batched NDJSON append; --id-prefix mints each row's id
tomlctl items update <file> <id> --json '{"status":"fixed"}' [--unset key]...
tomlctl items remove <file> <id>
tomlctl items next-id <file> --prefix R|O|E             # prefix is required — no default
tomlctl items apply  <file> --ops '[{"op":"add|update|remove", ...}, ...]' [--array NAME] [--on-stale abort|skip] [--id-prefix P]   # an update/remove op's `expect` object must still match its row; skip lists the stale ops under skipped_stale; --id-prefix mints each add op's id
tomlctl items find-duplicates <file> [--tier A|B|C] [--across <other>]   # dedup hygiene (read-only JSON array); --across runs tier A or B over the union of two ledgers
tomlctl items fingerprint <file> <id>                  # tier-B dedup_id of one stored row + the five fields that produced it
tomlctl items orphans  <file>                          # missing-file / symbol-missing / io-error / outside-repo / dangling-dep / instance-missing
tomlctl items sweep    <file> [--ids R1,R7] [--update] [--dry-run]  # re-run each item's `sweep` patterns, diff against `instances`; --update rewrites them
tomlctl items clusters <file> [--ids R1,R7]            # file-disjoint clusters + dependency batches + per-cluster lite_file_scope
tomlctl sweep -e <regex> [-e ...] [--exclude <glob>]... [--max-file-bytes N] [--max-hits N]  # regex hits over tracked files as file:line, with coverage + skip counts
tomlctl array-append   <file> <array> --json '{...}'                # append one record
tomlctl array-append   <file> <array> --ndjson -                    # batched append to e.g. rollback_events
tomlctl flow active list|add|remove [--slug <s>] [--branch <b>] [--worktree <w>] [--scope <glob>]...  # manage active-flow registry
tomlctl flow active touch --slug <s> [--dry-run]                    # refresh last_used timestamp for a flow in the registry
tomlctl flow doctor [--slug <s>] [--fix] [--dry-run]                # invariant checks across flows; always emits JSON; --fix regenerates sidecars / prunes stale registry entries / backfills an absent [artifacts].tasks key
tomlctl flow ensure-artifact --slug <s> --kind <k> [--bootstrap]    # report (or bootstrap execution-record) flow artifact + sidecar status
tomlctl flow find-plans [--dirs <d>...] [--strict-read]             # locate plan files under given dirs
tomlctl flow init --slug <s> --plan <path> [--branch <b>] [--scope <glob>]...   # seed context.toml + execution-record.toml + tasks.toml + active-flow entry (idempotent); emits {action, created, context_path, artifacts}
tomlctl flow list [--status <s>] [--branch <b>] [--active-only]     # enumerate flows under .claude/flows/
tomlctl flow resolve [--flow <s>] [--path <p>]... [--branch <b>] [--worktree <w>] [--with-staleness]  # 5-step flow resolution; emits {resolved, slug, source, artifacts, ...}
tomlctl flow stale --slug <s> [--threshold <duration>]              # check whether a flow is stale
tomlctl flow record --slug <s> --type <t> [--task <N>] [--set k=v]... [--set-file k=<path>]... [--ndjson <src>] [--dry-run]   # validated execution-record entry: mints E<n>, stamps date, derives task_ref
tomlctl blocks verify  <file>... [--block <marker-name>]...  # cross-file shared-block parity
tomlctl backlog check  --summary <s> [--area PATH] [--kind K] [--tag T]...  # is it already known? read-only graded verdict
tomlctl backlog check  --ndjson <src>                  # one {summary,kind?,area?,tags?} probe per line; one verdict row per probe
tomlctl backlog add    --summary <s> [--kind K] [--area PATH] [--evidence path:line]... [--context <how-to-work-around>]
tomlctl backlog add-many --ndjson <src> [--auto-base-sha] [--on-duplicate bump|skip|fail]   # one `add --json` payload per line; one lock, one write, all-or-nothing
tomlctl backlog list   [--open|--live] [--kind K] [--tag T]... [--area-prefix PATH] [--has-evidence] [--count]   # --live is open or promoted; plus the full --where-* query surface
tomlctl backlog show   <id>[,<id>...]                  # one item + its one-hop relations + its evidence listing; several ids print an array
tomlctl backlog relate B7 --to B3 --as relates-to|duplicates|supersedes   # duplicates dismisses B7, supersedes dismisses B3
tomlctl backlog triage --promote --to <flow-slug> B7   # --to must name a flow or plan (see --external / --allow-closed); or --dismiss --reason / --resolve --resolution / --reopen --rationale
tomlctl backlog triage --dismiss --reason <r> --expect-status open B7 B9   # move only ids still at that status; the rest land in skipped_stale
tomlctl backlog reconcile [--flow <s>] [--adopt] [--apply]   # bucket promoted items by their closing tasks; --adopt links by id mention, --apply resolves the ready ones
tomlctl backlog evidence dir <id>                      # per-item .claude/backlog-evidence/<id>/, created on demand
tomlctl backlog evidence audit [--strict] [--max-bytes N]   # unowned dirs, policy breaches, stale references
tomlctl backlog cluster --by all                       # group open items into candidate work scopes
tomlctl backlog compact [--older-than 90d] [--dry-run]  # ages resolved and dismissed items into [[compacted]]; open and promoted items never move
tomlctl tasks snapshot --slug <s>                      # one consistent read of a flow: rows, graph products, joined execution record, agent records
tomlctl tasks train --slug <s> [--checkpoint <id>]... [--ids N1,N2] [--granularity G]   # commit groups for the done rows with no commit, in commit order
tomlctl tasks show <N> --slug <s> --with absent        # the row's files that do not exist yet
tomlctl agents record --harness claude-code|codex|manual [-]   # one hook payload from stdin into the dispatching flow's agents.toml; the manual form of what the harness hooks do by running `glimpse hook`, which writes the same file in-process
tomlctl agents list --slug <s>                         # a flow's agent lifecycle records as a JSON array
tomlctl inputs list [--pending] [--kind K]... [--ledger L] [--flow <s>] [--scope <s>] [--item <id>]   # .claude/inputs.toml as {path, revision, inputs}
tomlctl inputs add --json '{"kind":"note",...}'        # append one record as `new`; prints its {id}
tomlctl inputs ack <id>... --by <command>              # new → acknowledged
tomlctl inputs handle <id>... --by <command> --note <text>   # new or acknowledged → handled
tomlctl inputs withdraw <id>...                        # withdraw new records, all or none
tomlctl inputs answer <question-id> [--pick <option>]... [--text <text>]   # answer a new question and mark it handled

# Output options (work before or after the subcommand): --select, --limit, --lines, --get, --template, -q,
#   --rows, --header, --max-chars, --omit, and the --where* filters

# Integrity flags (accepted after the subcommand name on any TOML-touching command):
#   --allow-outside           bypass the best-effort .claude/ containment guard (not a sandbox)
#   --no-write-integrity      suppress the <file>.sha256 sidecar on write
#   --verify-integrity        verify <file> against <file>.sha256 before any read
#   --strict-integrity        treat sidecar write failures as hard errors
```

`items list` also offers a full query surface — `--where / --where-not / --where-in / --where-has / --where-missing / --where-gt[e] / --where-lt[e] / --where-contains / --where-prefix / --where-suffix / --where-regex`, projections (`--select`, `--exclude`, `--pluck`), shaping (`--sort-by`, `--limit`, `--offset`, `--distinct`), aggregation (`--count`, `--count-by`, `--group-by`), and `--ndjson` output. All `KEY=VAL` right-hand sides accept typed prefixes (`@date:`, `@datetime:`, `@int:`, `@float:`, `@bool:`, `@string:` / `@str:`). See [references/query.md](../claude/skills/tomlctl/references/query.md#query-items-full-query-surface) for the full reference.

**Stdin input** (`-` sentinel on `--json` / `--ops` / `--ndjson` / `--defaults-json`): see [references/write.md stdin section](../claude/skills/tomlctl/references/write.md#stdin-input-for-large-json-payloads) for the full reference.

Most commands print JSON on stdout; `items next-id` prints a bare id, render verbs with `--stdout` print text, and `-q` prints nothing on success. Commands exit non-zero on failure.

A CLI write that changes a document refreshes an existing root `last_updated` to today (UTC); pass `--no-stamp` to leave it as it is.

## Design

- Uses [`toml 1.1.2+spec-1.1.0`](https://crates.io/crates/toml) with `preserve_order` for stable key layout.
- Whole-file parse → mutate → re-serialise. No format preservation (flow/ledger schemas forbid inline comments).
- Dates round-trip as TOML date literals; JSON strings matching `YYYY-MM-DD` are promoted to dates on write.
- **Integrity sidecar.** Every write emits `<file>.sha256` alongside the target, in standard `sha256sum` format (`<64-hex>  <basename>\n`), written atomically after the primary rename so an interleaved reader cannot see a torn pair. Pass `--no-write-integrity` to opt out. Pass `--verify-integrity` on any invocation to verify the target against its sidecar before every read — a missing sidecar or digest mismatch aborts with expected/actual hashes named in the error. `tomlctl` never auto-repairs; a mismatch means either an out-of-band edit or a corrupted sidecar, and a human should decide which. **Threat model.** The sidecar is a consistency check against accidental corruption and collaborative out-of-band edits — it is **not** a MAC or tamper-proof signature. An attacker with ledger write access can trivially rewrite the sidecar; hostile-actor threat models still require auditing the ledger's git history.

### Editing settings.json safely

Use `tomlctl json` as the safe edit path for `.claude/settings.json` rather than `tomlctl set` or `tomlctl set-json` — TOML writers refuse `.json` paths with a `kind=validation` error directing you to `tomlctl json set`. Quick reference:

```bash
tomlctl json get .claude/settings.json plansDirectory        # read current value
tomlctl json set .claude/settings.json plansDirectory --json '["docs/plans/"]'  # write
tomlctl json unset .claude/settings.json plansDirectory      # delete the key
```

**Whitespace-churn caveat.** `tomlctl json` uses `serde_json`'s pretty-printer (2-space indent, trailing newline). If `settings.json` was previously hand-formatted or written by a different tool with different indentation, a round-trip through `tomlctl json set` will normalise the entire file's whitespace. This shows as a large diff in `git diff` even when only one value changed — it is cosmetic and safe to commit.

**Sidecar.** `tomlctl json` does not maintain a `.sha256` sidecar for `settings.json` because the Claude Code harness writes that file out-of-band (e.g. on `/config`). Sidecar refresh is skipped by default on paths matching `**/settings.json`; pass `--no-write-integrity` explicitly on other `.json` files if you wish the same behaviour there.

## Contracts

### Dedup fingerprint

Every `items add`, `items update`, `items apply`, and `items add-many` auto-populates
a `dedup_id` field when:

- (add / add-many) the payload lacks `dedup_id`.
- (update / apply) the update touches any fingerprinted field (`file`, `summary`,
  `severity`, `category`, `symbol`) AND does not set `dedup_id` explicitly — where
  an `--unset` of such a field counts as a touch alongside a patch value for it.

The fingerprint is sha256 of `file|summary|severity|category|symbol` (each field
read as a string, empty-string for missing / non-string values; no additional
trimming or normalisation — field order is load-bearing and matches the tier-B
`items find-duplicates` hash exactly), truncated to 16 hex chars (64 bits).
Birthday-bound at ~4B items per scope; for adversarial inputs, set `dedup_id`
explicitly on the payload.

**Fingerprint diffs.** Three worked examples make the recompute vs preserve
contract concrete:

```
# (a) Fingerprinted-field change → new digest.
# Before:  {file:"a.rs", summary:"X", severity:"warning", category:"quality", symbol:"f"}
#          dedup_id = "30f663027c03dbf3"
# Patch:   items update <ledger> R1 --json '{"summary":"X2"}'
# After:   {file:"a.rs", summary:"X2", severity:"warning", category:"quality", symbol:"f"}
#          dedup_id = "c15bd8a7e1f492ab"   # recomputed — summary is fingerprinted

# (b) Non-fingerprinted-field change → digest preserved.
# Before:  {file:"a.rs", summary:"X", severity:"warning", category:"quality", symbol:"f",
#           status:"open", rounds:1}
#          dedup_id = "30f663027c03dbf3"
# Patch:   items update <ledger> R1 --json '{"status":"fixed","rounds":2}'
# After:   {file:"a.rs", summary:"X", severity:"warning", category:"quality", symbol:"f",
#           status:"fixed", rounds:2}
#          dedup_id = "30f663027c03dbf3"   # preserved — patch touched no fingerprinted field

# (c) Unset of a fingerprinted field the row carries → new digest.
# Before:  {file:"a.rs", summary:"X", severity:"warning", category:"quality", symbol:"f"}
#          dedup_id = "30f663027c03dbf3"
# Patch:   items update <ledger> R1 --unset symbol
# After:   {file:"a.rs", summary:"X", severity:"warning", category:"quality"}
#          dedup_id = "7b21c0f4de9a5163"   # recomputed — symbol now hashes as empty
```

(Digests above are illustrative shapes; the actual 16-hex value depends on the
exact field bytes fed to SHA-256 — run `tomlctl items fingerprint <ledger> R1` to
print the row's real digest alongside the five field values it hashed.
`items find-duplicates` does not answer this: every tier drops groups of fewer
than two members, so a unique row like `R1` produces `[]`.)

On update, four branches run in order — the first to match wins. Branches 2-4 turn
on whether the update *touches* a fingerprinted field, which the patch and the
`--unset` list decide jointly: a non-empty patch value for one counts, and so does
an `--unset` naming one the row actually carries. An `--unset` of a field the row
does not carry removes nothing and counts as no touch, as does an `--unset` of a
non-fingerprinted field.

1. **Patch explicitly sets `dedup_id` (non-empty string)**: preserve caller's value.
   Example: `items update <ledger> R1 --json '{"dedup_id":"explicit"}'` → the item
   ends up with `dedup_id = "explicit"` regardless of other patch fields or unsets.
2. **The update touches a fingerprinted field AND does not set `dedup_id`**:
   recompute from the post-merge, post-unset view of the five fingerprinted fields.
   An `--unset` key beats a patch value for the same field — the removals run after
   the merge, so the digest hashes the row that actually lands on disk.
3. **The update touches no fingerprinted field AND the existing item lacks
   `dedup_id`**: leave absent. Unrelated updates on legacy ledgers do NOT silently
   populate; use `tomlctl items backfill-dedup-id <file>` for the explicit upgrade
   path (added in a later release).
4. **The update touches no fingerprinted field AND the existing item HAS
   `dedup_id`**: preserve existing (nothing changed a fingerprint input, so the
   digest is still correct).

`items update --json '{"dedup_id":null}'` is treated as "patch didn't mention the
field" (branch 3 or 4, depending on existing state) — the less-surprising
semantics. Use an unset flag or an explicit non-empty value to force a change.

**Stale digests.** A row whose `dedup_id` was written before this unset-aware
recompute can carry a digest that hashes a field the row no longer has. `items backfill-dedup-id`
does not repair it: backfill only fills in a *missing* `dedup_id` and preserves any
value already present. Repair is two steps —

```bash
tomlctl items update <ledger> R1 --unset dedup_id
tomlctl items backfill-dedup-id <ledger>
```

Re-deriving leaves a correct digest byte-identical, so no detection step is needed
first; it does discard an explicitly-set `dedup_id`.

PROGRESS-LOG rendering is safe: `plan-update.md` hard-codes which columns make
it into rendered output, so `dedup_id` never leaks into user-facing progress
log lines despite being present on every new row.

`--dedupe-by <fields>` (on `items add` / `items add-many`) does NOT implicitly
include `dedup_id`. Callers wanting fingerprint-based dedup pass
`--dedupe-by dedup_id` explicitly. The dedupe pre-scan always runs BEFORE
auto-populate, so a payload's auto-populated `dedup_id` never influences its
own pre-scan — preserving `--dedupe-by`'s "raw-equality-on-named-fields"
contract.

To disable auto-populate globally (rollback lever): `TOMLCTL_NO_DEDUP_ID=1`.
Any value (even empty) disables the hook; unset the env var to re-enable.

`items find-duplicates --across <other>` runs tier A or B over the union of two
ledgers, tagging each JSON output entry with `source_file` (the basename of its
origin ledger). The tag is applied at JSON-emit time and never written to either
on-disk ledger. Tier C is file-scoped by design (its line-window grouping
assumes one source file) and errors under `--across`:

```
tier C is file-scoped; use --tier A or --tier B with --across
```

### File state contract

`tomlctl` distinguishes three states for a target file:

| State                     | Default mode                              | `--strict-read` mode         |
|---------------------------|-------------------------------------------|------------------------------|
| Missing                   | Empty-default (e.g. `items next-id --prefix R` → `R1`) | Error `kind=not_found`       |
| Zero-byte                 | Treated as a minimal valid doc            | Same (no error)              |
| Exists but malformed TOML | Error `kind=parse`                        | Same                         |

Three read surfaces have a "missing file → silent default" branch, and
`--strict-read` turns each into `kind=not_found`:

- `items next-id --prefix <P>` returns `<P>1`, a bootstrapping fast path for
  flows that mint the first id before the ledger file exists.
- The `backlog` read verbs (`check`, `list`, `show`, `cluster`, `evidence dir`,
  `evidence audit`) read a missing `.claude/backlog.toml` as an empty store,
  because the first capture in a repo runs `backlog check` before anything
  exists.
- `agents list` returns `[]` for a missing `agents.toml`, the state of a flow
  whose hooks have not fired yet.

Every other read subcommand (`parse`, `get`, `validate`, `json get`,
`items list`, `items get`, `items find-duplicates`, `items fingerprint`,
`items orphans`, `items clusters`) already errors on a missing file with
`kind=not_found` — `--strict-read` is a no-op there, but the flag is accepted
on every read subcommand so a caller can pass it uniformly without branching
on subcommand name. The `tasks` read verbs (`show`, `list`, `edges`, `ready`,
`batches`, `closure`, `check`, `render`, `snapshot`) belong to this group too;
`snapshot`'s companion files stay optional under the flag. Four `flow` read verbs give
the flag a meaning of their own: `flow stale` and an explicit
`flow resolve --flow` error on a missing `context.toml`, `flow find-plans` on
a missing configured plan directory, and `flow list` on an unreadable
`context.toml` it would otherwise skip. Two exceptions: `items sweep` carries the write-side
integrity bundle (it can `--update` the ledger) and rejects `--strict-read`,
and the standalone `sweep` reads no TOML and takes no integrity flag at all.

Pass `--strict-read` when an agent needs to distinguish "no matches in an
existing ledger" from "ledger does not exist" — e.g. when a flow expects a
specific file to have been bootstrapped by `/plan-new` or `/implement` before
proceeding.

`--strict-read` fires **before**
`--verify-integrity`: a missing file under both flags produces
`kind=not_found`, not `kind=integrity` (the sidecar check would also have
failed, but the underlying state is "file missing", not "file tampered").

### Capabilities feature list

`tomlctl capabilities` emits a stable JSON description of this binary so
downstream flow-command templates can feature-gate at boot without parsing
`--help` prose:

```json
{
  "version": "0.15.0",
  "features": ["count_distinct", "raw", "lines", "infer_prefix",
               "dedupe_by", "dedup_id_auto", "find_duplicates_across",
               "fingerprint", "capabilities", "error_format_json",
               "strict_read", "dry_run", "backfill_dedup_id",
               "integrity_refresh", "agent_context", "flow_resolve",
               "flow_active", "flow_doctor", "flow_init",
               "flow_ensure_artifact", "flow_envelope_build", "flow_stale",
               "flow_find_plans", "flow_list", "flow_render_progress_log",
               "json_ops", "backlog_capture", "backlog_add_many",
               "backlog_check",
               "backlog_cluster", "backlog_compact", "backlog_evidence",
               "backlog_list", "backlog_show", "backlog_relate",
               "backlog_triage", "backlog_triage_expect",
               "backlog_reconcile",
               "tasks_import_plan", "tasks_add",
               "tasks_add_many", "tasks_update", "tasks_remove",
               "tasks_show", "tasks_list", "tasks_edges", "tasks_ready",
               "tasks_batches", "tasks_closure", "tasks_check",
               "tasks_render", "tasks_snapshot", "agents_record",
               "agents_list", "inputs", "items_apply_expect", "sweep",
               "items_sweep", "items_clusters", "orphans_instances",
               "output_options", "next_id_bare", "id_prefix",
               "auto_last_updated", "multi_id", "path_wildcard",
               "rows_header", "global_where", "array_predicates",
               "max_chars", "omit", "template_width", "list_limited",
               "field_flags", "flow_record", "multi_set",
               "context_updated_stamp", "apply_id_prefix",
               "id_high_water", "tasks_train", "show_absent",
               "update_ref", "backlog_check_batch", "suggestions"],
  "subcommands": ["parse", "get", "set", "set-json", "validate",
                  "items", "blocks", "array-append", "capabilities",
                  "integrity", "flow", "json", "backlog", "tasks",
                  "agents", "inputs", "sweep"],
  "global_flags": {
    "--error-format": {"type": "enum", "required": false, "default": "text", "values": ["text","json"], "repeatable": false},
    "--select":       {"type": "string", "required": false, "repeatable": false},
    "--limit":        {"type": "string", "required": false, "repeatable": false},
    "--lines":        {"type": "bool",   "required": false, "values": ["true","false"], "repeatable": false},
    "--get":          {"type": "string", "required": false, "repeatable": false},
    "--template":     {"type": "string", "required": false, "repeatable": false},
    "--quiet":        {"type": "bool",   "required": false, "values": ["true","false"], "repeatable": false},
    "--rows":         {"type": "string", "required": false, "repeatable": false},
    "--header":       {"type": "bool",   "required": false, "values": ["true","false"], "repeatable": false},
    "--max-chars":    {"type": "string", "required": false, "repeatable": false},
    "--omit":         {"type": "string", "required": false, "repeatable": false},
    "--where":        {"type": "string", "required": false, "repeatable": true}
  },
  "commands": {
    "items": {
      "subcommands": {
        "list": {
          "flags": {
            "<file>":  {"type": "string", "required": true,  "repeatable": false},
            "--count": {"type": "bool",   "required": false, "values": ["true","false"], "repeatable": false},
            "--array": {"type": "string", "required": false, "default": "items", "repeatable": false},
            "--where": {"type": "string", "required": false, "repeatable": true},
            "--pluck": {"type": "string", "required": false, "repeatable": false}
          },
          "mutex_groups": [["count","count_by","group_by","pluck","count_distinct"]]
        }
      }
    }
  }
}
```

(The `commands` slice above is abbreviated — every subcommand has an entry; run `tomlctl capabilities` for the full tree. `global_flags` is abbreviated too: it lists every `--where*` filter, of which only `--where` is shown.) Each flag entry carries `type` (`string` / `bool` / `enum`), `required`, `repeatable`, and optional `default` / `values`. `mutex_groups` lists clap `ArgGroup` mutex sets so an agent can pre-validate a flag combination before invoking the binary. `global_flags` uses the same entry shape for the root's flags, which every subcommand accepts and which no `commands` entry repeats.

Stability contract:

- Each entry in `features` is stable across patch versions within a minor
  release. Removing an entry is a breaking change that ships in a minor
  version bump.
- New user-facing flags add new `features` entries; do NOT version-qualify
  (the `version` field is the release marker).
- `subcommands` mirrors the top-level `Cmd` enum's kebab-case names.
- `version` reads from `CARGO_PKG_VERSION` via `env!`, so `tomlctl/Cargo.toml`
  is the single source of truth.

**`commands` key — best-effort introspection, not a stable contract.** The `version`, `features`, and `subcommands` top-level keys are stable across minor releases (additive only — new entries may appear, none are removed within a minor line). The `commands` key (added in 0.4.0, gated by the `agent_context` feature) is **best-effort runtime introspection** of the clap command tree. Its shape (`flags`, `mutex_groups`, nested `subcommands`) is intended to be additive across minor releases, but specific edge cases — particularly the `flags.<name>.type` mapping for repeatable / non-string Append flags — depend on clap-4 implementation details and may shift if clap evolves its `ArgAction` surface. Consumers MUST treat unknown `type` values as opaque strings rather than relying on a closed set of `{string, bool, enum, count}`. To audit the `commands` schema for a given release, run `tomlctl capabilities` against that binary directly rather than caching the shape across upgrades.

Feature meanings:

| Feature | What it enables |
|---|---|
| `count_distinct` | `--count-distinct <FIELD>` on `items list` |
| `raw` | `--raw` scalar emit on `get` and on single-value `items list` shapes |
| `lines` | `--lines` on `items list --pluck` prints one value per line; on every other command `--lines` is the global compact NDJSON form listed under `output_options` |
| `infer_prefix` | `items next-id --infer-from-file` |
| `dedupe_by` | `--dedupe-by <FIELDS>` on `items add` / `items add-many` |
| `dedup_id_auto` | auto-populate `dedup_id` in every write funnel |
| `find_duplicates_across` | `items find-duplicates --across <other>` cross-ledger tier A/B |
| `fingerprint` | `items fingerprint <file> <ID>` — one stored row's tier-B `dedup_id` plus the five fingerprinted field values it hashed, including for a unique row `find-duplicates` cannot report |
| `capabilities` | this subcommand itself |
| `error_format_json` | `--error-format json` global flag + `ErrorKind` taxonomy |
| `strict_read` | `--strict-read` on every read subcommand except `items sweep` (write-side integrity bundle) and the standalone `sweep` (no integrity flags) |
| `dry_run` | `--dry-run` on the write subcommands that support previewing mutations, including `set`, `set-json`, `array-append`, `items add`, `items add-many`, `items update`, `items remove`, `items apply`, `items backfill-dedup-id`, `flow init`, `flow ensure-artifact`, `flow doctor`, `flow active add`, `flow active remove`, `flow active touch`, `json set`, `json unset`, `backlog add`, `backlog add-many`, `backlog compact`, `tasks import-plan`, and `items sweep --update` |
| `backfill_dedup_id` | `items backfill-dedup-id <file>` |
| `integrity_refresh` | `tomlctl integrity refresh <file>` — sidecar bootstrap / recovery primitive |
| `agent_context` | `tomlctl capabilities .commands` emits per-subcommand flag schema (type / required / default / values / repeatable + mutex_groups) for runtime introspection without parsing `--help` prose |
| `flow_resolve` | `flow resolve` — the 6-step active-flow resolution, emitting the JSON flow envelope |
| `flow_active` | `flow active list` / `add` / `remove` — the `.claude/active-flow.toml` registry |
| `flow_doctor` | `flow doctor` — cross-flow invariant checks; `--fix` repairs sidecar mismatches, prunes stale registry entries, and backfills an absent `[artifacts].tasks` key |
| `flow_init` | `flow init --slug <SLUG> --plan <PATH>` — idempotent bootstrap of a new flow |
| `flow_ensure_artifact` | `flow ensure-artifact --kind <KIND>` — report a flow artifact, and with `--bootstrap` materialise it |
| `flow_envelope_build` | `flow envelope build --command <CARRIER>` — emit the canonical `flow-bootstrap` input envelope on stdout |
| `flow_stale` | `flow stale --slug <SLUG> --threshold <DURATION>` — staleness verdict for a flow's `context.toml` |
| `flow_find_plans` | `flow find-plans` — discover plan markdown under the configured plans directories or `--dirs` |
| `flow_list` | `flow list` — enumerate every flow under `.claude/flows/`, filterable by `--status` / `--branch` / `--active-only` |
| `flow_render_progress_log` | `flow render-progress-log --slug <SLUG>` — regenerate a flow's derived `PROGRESS-LOG.md` from its `execution-record.toml`; `--stdout` previews without writing |
| `json_ops` | `json get` / `set` / `unset` — dotted-path read and write against JSON files such as `.claude/settings.json` |
| `backlog_capture` | `backlog add` — capture a discovery into the repo-scoped backlog store |
| `backlog_add_many` | `backlog add-many --ndjson <SRC>` — capture a batch, one `backlog add --json` payload per line, under one lock and one write; a malformed line, an unknown key or a refused row aborts the whole batch, naming its line |
| `backlog_check` | `backlog check --summary <TEXT>` — read-only graded verdict on whether a discovery is already known, before minting it; `in-flight` when the match is a `promoted` item, with the flow it is promoted to |
| `backlog_cluster` | `backlog cluster --by <VIEW>` — group open items into candidate work scopes |
| `backlog_compact` | `backlog compact --older-than <DURATION>` — age resolved and dismissed items into `[[compacted]]`; `open` and `promoted` items are never touched |
| `backlog_evidence` | `backlog evidence dir` / `audit` — per-item evidence directories under `.claude/backlog-evidence/` |
| `backlog_list` | `backlog list` — query the store, with `--open` / `--live` (open or promoted) / `--area-prefix` / `--has-evidence` on top of the shared filter and projection surface |
| `backlog_show` | `backlog show <ID>` — one item with its one-hop relation neighbourhood and evidence listing; `backlog show <ID>,<ID>...` prints an array of those objects, and an unknown id fails the whole call |
| `backlog_relate` | `backlog relate <A> --to <ID> --as <KIND>` — write a typed edge between two items |
| `backlog_triage` | `backlog triage <ID>... --promote` / `--dismiss` / `--resolve` / `--reopen` — transition items out of (or back into) `open`; `--promote --to` must name an existing flow or plan and stores a plan some flow binds as that flow's slug, `--external` stores `external:<REF>` unresolved, and `--allow-closed` accepts a flow at `review` or `complete`. Resolving or dismissing a promoted item keeps its `promoted` / `promoted_to` claim |
| `backlog_triage_expect` | `backlog triage --expect-status <STATUS>` — move only the ids still at that status; the envelope lists the moved ids under `applied` and each other id under `skipped_stale` with its expected and found status |
| `backlog_reconcile` | `backlog reconcile [--flow <SLUG>]` — join each `promoted` item to the tasks that close it in its flow's `tasks.toml` and bucket it as `ready`, `in-progress`, `stalled`, `unlinked`, `orphaned`, `dangling` or `external`, each entry with a `reason`; read-only by default. `--adopt` links an item no task links yet to the tasks whose prose names its id and lists the flows in `render_needed`, which must be re-rendered with `tasks render` before the next import; `--apply` resolves the `ready` items, recording `resolved_flow`, `resolved_tasks` and `resolved_commits` |
| `tasks_import_plan` | `tasks import-plan --slug <SLUG>` — upsert a plan's `## Tasks`, `## Execution Policy` and `## Dependency Graph` into the store, keyed on each row's `ref`; `--reconcile-record` adopts the execution record's completions |
| `tasks_add` | `tasks add --title <TEXT> --effort <S\|M\|L>` — append one row, refusing a dangling dependency target or a cycle before writing |
| `tasks_add_many` | `tasks add-many --ndjson <SRC>` — append a batch of rows all-or-nothing |
| `tasks_update` | `tasks update <N>[,<N>...] --status <STATUS>` — patch the mutable fields of one row, or of several in one locked write that prints `{"ok":true,"results":[{"id":N,"ref":"...","changed":[...]},...]}` (one id prints `{"ok":true,"id":N,"ref":"...","changed":[...]}`); an unknown id writes nothing; `ref` moves only under an explicit `--ref`, which takes a single id |
| `tasks_remove` | `tasks remove <N>` — retire one row, the only verb that deletes; a settled row, or one other rows depend on, needs `--force`, which splices its dependencies into every dependent |
| `tasks_show` | `tasks show <N>[,<N>...] --with body,files,deps` — one row, or an array of rows for several ids; the single-id call is the fetch-by-id form an orchestrator hands a dispatched agent in place of pasted prose |
| `tasks_list` | `tasks list` — query rows with the full `items list` predicate, projection and aggregation surface |
| `tasks_edges` | `tasks edges --kind needs\|coupling\|overlap` — the edge list, or Graphviz DOT under `--dot` |
| `tasks_ready` | `tasks ready --in-flight <N1,N2,...>` — the dispatchable frontier, which rows a file claim holds, and what unblocks once the round lands |
| `tasks_batches` | `tasks batches` — the graph's Kahn layers, each sorted ascending |
| `tasks_closure` | `tasks closure --checkpoint <ID>` / `--task <N> --up` / `--down` — a checkpoint group's task set or one task's transitive closure, plus its maximal elements |
| `tasks_check` | `tasks check --in-flight <N1,N2,...>` — the store's invariant checks; any `error`-class finding exits 1, and `--plan` reports render drift as a warning, so gate drift on `tasks render --check` |
| `tasks_render` | `tasks render` — rewrite the plan's `## Execution Policy`, `## Tasks` and `## Dependency Graph` from the store; `--stdout` previews and `--check` reports drift without writing |
| `tasks_snapshot` | `tasks snapshot --slug <SLUG>` — one read-only JSON view of a flow for a viewer: rows, Kahn layers, the `tasks ready` frontier, edges, checkpoints, the execution record joined to its rows, and the agent records; absent sibling files read as empty, and `revision` fingerprints the raw input bytes |
| `agents_record` | `agents record --harness <HARNESS> [PAYLOAD]` — record one hook payload (an agent starting, idling or stopping) into the dispatching flow's `.claude/flows/<SLUG>/agents.toml`; the manual form of what `glimpse hook` does in-process; never run by carriers or sub-agents |
| `agents_list` | `agents list --slug <SLUG>` — a flow's agent lifecycle records as a JSON array |
| `inputs` | `inputs list` / `add` / `ack` / `handle` / `withdraw` / `answer` — the repo-scoped `.claude/inputs.toml` store of captures, requests, notes, questions and answers, each moving `new` → `acknowledged` → `handled` (or withdrawn while `new`) |
| `items_apply_expect` | a per-op `expect` object on `items apply` update and remove ops, mapping field to expected value (`null` for an absent field), checked against the row as stored; `--on-stale abort` (the default) fails the whole batch naming every stale op, `--on-stale skip` applies the rest and lists the dropped ops under `skipped_stale` |
| `sweep` | `sweep -e <REGEX>...` — regex hits over the repo's tracked files as sorted `file:line` sites, with `skipped` counts, `truncated` and `coverage_complete`; `.claude/**` and `docs/plans/**` are excluded by default |
| `items_sweep` | `items sweep <file>` — re-run each item's stored `sweep` patterns and diff the hits against its `instances` (`new` / `gone` / `kept` / `unverified`); `--update` rewrites an open item's `instances` in listed order, appending new sites, and refuses while the run is truncated or an anchor is unverified for any reason but `excluded` |
| `items_clusters` | `items clusters <file> --ids <R1,R7,...>` — file-disjoint clusters over `file` plus `instances`, layered by `depends_on` into batches, each cluster carrying `lite_file_scope`; out-of-selection dependencies land in `dropped_deps` and a cycle is refused |
| `orphans_instances` | `items orphans` reports an `instance-missing` class for every `instances` anchor that does not resolve, with `reason` one of `missing-file`, `symbol-missing`, `io-error`, `outside-repo`, `unparseable` |
| `output_options` | the global output flags `--select`, `--limit`, `--lines`, `--get`, `--template` and `-q`/`--quiet` (later releases add `--rows`, `--header`, `--where*`, `--omit` and `--max-chars`, each with its own row), accepted before or after the subcommand on every command and listed once under the root `global_flags` key — a command's accepted flags are its `.commands` flags plus `global_flags` |
| `next_id_bare` | `items next-id` prints the bare id (`R23`), with no JSON quotes |
| `id_prefix` | `items add --id-prefix <P>` / `items add-many --id-prefix <P>` — mint each new row's id inside the locked write; the envelope carries `id` (add) or `ids` (add-many), a dedupe skip reports the matched row's `id`, and a payload that already carries an `id` is refused |
| `auto_last_updated` | CLI writes through `set`, `set-json`, `array-append` and `items add` / `add-many` / `update` / `remove` / `apply` / `sweep --update` / `backfill-dedup-id` refresh an existing root `last_updated` to today (UTC) when they change anything else; `--no-stamp` opts out, and dry-run previews are unstamped |
| `multi_id` | `tasks update`, `tasks show` and `backlog show` take a comma- or space-separated id list; one id prints exactly the single-id output |
| `path_wildcard` | a `*` segment in a `--select`, `--get` or template path matches every element of an array or every value of an object; `--select` keys the matches by the path string as an array, `--get` prints one match per line, a template placeholder joins them with commas; a path that resolves nothing suggests up to three dotted paths one level deeper |
| `rows_header` | global `--rows <PATH>` turns the array at PATH of a single-object report into the rows, with the rest as the header; global `--header` shapes a row report's header as one object; the two are exclusive and both are refused on the list verbs |
| `global_where` | the `--where*` filters are global: they filter any row report (including one re-rooted by `--rows`) before `--limit`; on a single object without `--rows` they are a `kind=validation` error, and `--where*` given on both sides of the subcommand is refused |
| `array_predicates` | a `--where*` filter on an array field matches element by element — any element for `--where`, `-in`, the string and comparison filters, no element for `--where-not`; a dotted key absent flat is navigated as a path |
| `max_chars` | global `--max-chars <N>` cuts every string value longer than N characters to its first N followed by `…(+K)`, K being the number cut |
| `omit` | global `--omit <P1,P2,...>` drops dotted paths from each row and the header, or from the single object; exclusive with `--select`, `--get` and `--template` |
| `template_width` | a template placeholder `{path:N}` truncates its value as `--max-chars N` does |
| `list_limited` | the list verbs report a `limited` header (`{"shown":n,"total":N}`) when `--limit` cut rows; streamed `--ndjson`, `--pluck` and `--raw` output reports it on stderr instead |
| `field_flags` | `--set KEY=VALUE` (string), `--set-json KEY=JSON` and `--set-file KEY=PATH` (file text, `-` for stdin) build the payload on `items add`, `items update`, `array-append` and `flow record`, merging over an optional `--json`; a dotted key nests and a repeated key is refused |
| `flow_record` | `flow record --slug <SLUG> --type <TYPE>` — append a validated execution-record entry: id minted as `E<n>`, `date` defaulted to today (UTC), `task_ref` filled from `--task <N>`, over-cap text truncated and unsafe `files` dropped (both reported), `scope_warnings` for files outside the flow's scope; `--ndjson` appends a batch all-or-nothing |
| `multi_set` | `set <FILE> --set PATH=VALUE...` — several scalars in one write, alongside or instead of the positional pair |
| `context_updated_stamp` | `set`, `set-json` and `array-append` on a `context.toml` refresh its existing root `updated` to today (UTC) unless the write set it; `--no-stamp` opts out |
| `apply_id_prefix` | `items apply --id-prefix <P>` mints an id for each add op, in op order inside the lock, and reports them as `ids`; an add op that carries an `id` is refused |
| `id_high_water` | removing an id records its number in the root `[id_high_water]` table, so minting never reuses a removed top id |
| `tasks_train` | `tasks train --slug <SLUG>` — the commit groups for the `done` rows with no `commit`, by `commit_granularity`, merged on any shared file and on any dependency cycle between groups, in commit order; `shared_with_pending` names a group file a row not yet `done` claims |
| `show_absent` | `tasks show <N> --with absent` — the row's `files` that do not exist under the repository root |
| `update_ref` | `tasks update` envelopes carry each row's `ref` |
| `backlog_check_batch` | `backlog check --ndjson <SRC>` — one `{summary,kind?,area?,tags?}` probe per line, read against one load of the store; one `results` row per probe with its `verdict`, `dedup_id` and up to five `candidates` |
| `suggestions` | a mistyped subcommand or flag gets clap's `did you mean` hint; hidden aliases accept `read` for `parse`, `--as related` for `relates-to`, and `--id` / `--ids` beside the positional ids of `tasks show`, `tasks update`, `backlog show` and `backlog triage` |
