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
tomlctl set-json    <file> <path> --json <json>       # array / object / scalar
tomlctl json get    <file> <path>                     # read a value via the json subcommand
tomlctl json set    <file> <path> --json <value>      # write a value (array / object / scalar)
tomlctl json unset  <file> <path>                     # delete a key at path
tomlctl validate    <file>                            # parse-check
tomlctl items list  <file> [--status X] [--category Y] [--newer-than YYYY-MM-DD] [--file PATH] [--count]
tomlctl items get   <file> <id>
tomlctl items add   <file> --json '{"id":"R7",...}'
tomlctl items add-many <file> --defaults-json '{...}' --ndjson -    # batched NDJSON append
tomlctl items update <file> <id> --json '{"status":"fixed"}' [--unset key]...
tomlctl items remove <file> <id>
tomlctl items next-id <file> --prefix R|O|E             # prefix is required — no default
tomlctl items apply  <file> --ops '[{"op":"add|update|remove", ...}, ...]' [--array NAME]
tomlctl items find-duplicates <file> [--tier A|B|C] [--across <other>]   # dedup hygiene (read-only JSON array); --across runs tier A or B over the union of two ledgers
tomlctl items orphans  <file>                          # missing-file / symbol-missing / dangling-dep
tomlctl array-append   <file> <array> --json '{...}'                # append one record
tomlctl array-append   <file> <array> --ndjson -                    # batched append to e.g. rollback_events
tomlctl flow active list|add|remove [--slug <s>] [--branch <b>] [--worktree <w>] [--scope <glob>]...  # manage active-flow registry
tomlctl flow active touch --slug <s> [--dry-run]                    # refresh last_used timestamp for a flow in the registry
tomlctl flow doctor [--slug <s>] [--fix] [--dry-run]                # invariant checks across flows; always emits JSON; --fix regenerates sidecars / prunes stale registry entries
tomlctl flow ensure-artifact --slug <s> --kind <k> [--bootstrap]    # report (or bootstrap execution-record) flow artifact + sidecar status
tomlctl flow find-plans [--dirs <d>...] [--strict-read]             # locate plan files under given dirs
tomlctl flow init --slug <s> --plan <path> [--branch <b>] [--scope <glob>]...   # seed context.toml + execution-record.toml + active-flow entry (idempotent)
tomlctl flow list [--status <s>] [--branch <b>] [--active-only]     # enumerate flows under .claude/flows/
tomlctl flow resolve [--flow <s>] [--path <p>]... [--branch <b>] [--worktree <w>] [--with-staleness]  # 5-step flow resolution; emits {resolved, slug, source, artifacts, ...}
tomlctl flow stale --slug <s> [--threshold <duration>]              # check whether a flow is stale
tomlctl blocks verify  <file>... [--block <marker-name>]...  # cross-file shared-block parity
tomlctl backlog check  --summary <s> [--area PATH] [--kind K] [--tag T]...  # is it already known? read-only graded verdict
tomlctl backlog add    --summary <s> [--kind K] [--area PATH] [--evidence path:line]... [--context <how-to-work-around>]
tomlctl backlog list   [--open] [--kind K] [--tag T]... [--area-prefix PATH] [--has-evidence] [--count]   # plus the full --where-* query surface
tomlctl backlog show   <id>                            # one item + its one-hop relations + its evidence listing
tomlctl backlog relate B7 --to B3 --as relates-to|duplicates|supersedes   # duplicates dismisses B7, supersedes dismisses B3
tomlctl backlog triage --promote --to tomlctl-backlog-capture B7   # or --dismiss --reason / --resolve --resolution / --reopen --rationale
tomlctl backlog evidence dir <id>                      # per-item .claude/backlog-evidence/<id>/, created on demand
tomlctl backlog evidence audit [--strict] [--max-bytes N]   # unowned dirs, policy breaches, stale references
tomlctl backlog cluster --by all                       # group open items into candidate work scopes
tomlctl backlog compact [--older-than 90d] [--dry-run]  # ages decided items into [[compacted]]; open items never move

# Integrity flags (accepted after the subcommand name on any TOML-touching command):
#   --allow-outside           bypass the best-effort .claude/ containment guard (not a sandbox)
#   --no-write-integrity      suppress the <file>.sha256 sidecar on write
#   --verify-integrity        verify <file> against <file>.sha256 before any read
#   --strict-integrity        treat sidecar write failures as hard errors
```

`items list` also offers a full query surface — `--where / --where-not / --where-in / --where-has / --where-missing / --where-gt[e] / --where-lt[e] / --where-contains / --where-prefix / --where-suffix / --where-regex`, projections (`--select`, `--exclude`, `--pluck`), shaping (`--sort-by`, `--limit`, `--offset`, `--distinct`), aggregation (`--count`, `--count-by`, `--group-by`), and `--ndjson` output. All `KEY=VAL` right-hand sides accept typed prefixes (`@date:`, `@datetime:`, `@int:`, `@float:`, `@bool:`, `@string:` / `@str:`). See [references/query.md](../claude/skills/tomlctl/references/query.md#query-items-full-query-surface) for the full reference.

**Stdin input** (`-` sentinel on `--json` / `--ops` / `--ndjson` / `--defaults-json`): see [references/write.md stdin section](../claude/skills/tomlctl/references/write.md#stdin-input-for-large-json-payloads) for the full reference.

All commands print JSON on stdout, exit non-zero on failure.

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
- (update / apply) the patch touches any fingerprinted field (`file`, `summary`,
  `severity`, `category`, `symbol`) AND does not set `dedup_id` explicitly.

The fingerprint is sha256 of `file|summary|severity|category|symbol` (each field
read as a string, empty-string for missing / non-string values; no additional
trimming or normalisation — field order is load-bearing and matches the tier-B
`items find-duplicates` hash exactly), truncated to 16 hex chars (64 bits).
Birthday-bound at ~4B items per scope; for adversarial inputs, set `dedup_id`
explicitly on the payload.

**Fingerprint diffs.** Two worked examples make the recompute vs preserve
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
```

(Digests above are illustrative shapes; the actual 16-hex value depends on the
exact field bytes fed to SHA-256 — rerun `tomlctl items find-duplicates --tier B`
against the payload to confirm.)

On update, four branches run in order — the first to match wins:

1. **Patch explicitly sets `dedup_id` (non-empty string)**: preserve caller's value.
   Example: `items update <ledger> R1 --json '{"dedup_id":"explicit"}'` → the item
   ends up with `dedup_id = "explicit"` regardless of other patch fields.
2. **Patch touches a fingerprinted field AND does not set `dedup_id`**: recompute
   from the merged (patch-over-existing) view of the five fingerprinted fields.
3. **Patch does NOT touch any fingerprinted field AND the existing item lacks
   `dedup_id`**: leave absent. Unrelated updates on legacy ledgers do NOT silently
   populate; use `tomlctl items backfill-dedup-id <file>` for the explicit upgrade
   path (added in a later release).
4. **Patch does NOT touch any fingerprinted field AND the existing item HAS
   `dedup_id`**: preserve existing (the patch can't have changed any fingerprint
   input, so the digest is still correct).

`items update --json '{"dedup_id":null}'` is treated as "patch didn't mention the
field" (branch 3 or 4, depending on existing state) — the less-surprising
semantics. Use an unset flag or an explicit non-empty value to force a change.

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
| Missing                   | Empty-default (e.g. `items next-id --prefix R` → `"R1"`) | Error `kind=not_found`       |
| Zero-byte                 | Treated as a minimal valid doc            | Same (no error)              |
| Exists but malformed TOML | Error `kind=parse`                        | Same                         |

Today the only read subcommand with a "missing file → silent default" branch
is `items next-id --prefix <P>`, which returns `"<P>1"` as a bootstrapping fast
path for flows that mint the first id before the ledger file exists. Every
other read subcommand (`parse`, `get`, `validate`, `items list`, `items get`,
`items find-duplicates`, `items orphans`) already errors on a missing file with
`kind=not_found` — `--strict-read` is a no-op there, but the flag is accepted
on every read subcommand so a caller can pass it uniformly without branching
on subcommand name.

Pass `--strict-read` when an agent needs to distinguish "no matches in an
existing ledger" from "ledger does not exist" — e.g. when a flow expects a
specific file to have been bootstrapped by `/plan-new` or `/implement` before
proceeding.

`--strict-read` fires **before** `--verify-integrity`: a missing file under
both flags produces `kind=not_found`, not `kind=integrity` (the sidecar check
would also have failed, but the underlying state is "file missing", not
"file tampered").

### Capabilities feature list

`tomlctl capabilities` emits a stable JSON description of this binary so
downstream flow-command templates can feature-gate at boot without parsing
`--help` prose:

```json
{
  "version": "0.6.0",
  "features": ["count_distinct", "raw", "lines", "infer_prefix",
               "dedupe_by", "dedup_id_auto", "find_duplicates_across",
               "capabilities", "error_format_json", "strict_read",
               "dry_run", "backfill_dedup_id", "integrity_refresh",
               "agent_context", "flow_resolve", "flow_active",
               "flow_doctor", "flow_init", "flow_ensure_artifact",
               "flow_envelope_build", "flow_stale", "flow_find_plans",
               "json_ops", "backlog_capture", "backlog_check",
               "backlog_cluster", "backlog_compact", "backlog_evidence",
               "backlog_list", "backlog_show", "backlog_relate", "backlog_triage"],
  "subcommands": ["parse", "get", "set", "set-json", "validate",
                  "items", "blocks", "array-append", "capabilities",
                  "integrity", "flow", "json", "backlog"],
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

(The `commands` slice above is abbreviated — every subcommand has an entry; run `tomlctl capabilities` for the full tree.) Each flag entry carries `type` (`string` / `bool` / `enum`), `required`, `repeatable`, and optional `default` / `values`. `mutex_groups` lists clap `ArgGroup` mutex sets so an agent can pre-validate a flag combination before invoking the binary.

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
| `lines` | `--lines` newline-per-value emit on `items list --pluck` |
| `infer_prefix` | `items next-id --infer-from-file` |
| `dedupe_by` | `--dedupe-by <FIELDS>` on `items add` / `items add-many` |
| `dedup_id_auto` | auto-populate `dedup_id` in every write funnel |
| `find_duplicates_across` | `items find-duplicates --across <other>` cross-ledger tier A/B |
| `capabilities` | this subcommand itself |
| `error_format_json` | `--error-format json` global flag + `ErrorKind` taxonomy |
| `strict_read` | `--strict-read` on every read subcommand |
| `dry_run` | `--dry-run` on all 9 write subcommands: `set`, `set-json`, `array-append`, `items add`, `items add-many`, `items update`, `items remove`, `items apply`, `items backfill-dedup-id` |
| `backfill_dedup_id` | `items backfill-dedup-id <file>` |
| `integrity_refresh` | `tomlctl integrity refresh <file>` — sidecar bootstrap / recovery primitive |
| `agent_context` | `tomlctl capabilities .commands` emits per-subcommand flag schema (type / required / default / values / repeatable + mutex_groups) for runtime introspection without parsing `--help` prose |
