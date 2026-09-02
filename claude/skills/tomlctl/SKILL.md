---
name: tomlctl
description: "Read, write, query, batch-edit, and validate TOML files used by Claude Code flows — context.toml, review-ledger.toml, optimise-findings.toml, execution-record.toml, plan-review-findings.toml — and their per-row [[items]] arrays. Verbs: read/parse/get, query/filter/list/count/group-by/pluck, write/set/set-json, append/array-append, items add/add-many/update/remove/apply/backfill-dedup-id, validate, integrity refresh/verify, dry-run preview, dedupe. Use this for any TOML mutation in a flow command — never line-edit ledger arrays-of-tables. Outputs JSON; supports stdin via `-` sentinel for ops/json/ndjson payloads. Single agent-native CLI for all flow-TOML I/O on Windows and Linux."
---

# tomlctl

> This document is the authoritative tomlctl reference. The top-level `tomlctl/README.md` is a short human tour that intentionally defers here for anything beyond the quick-tour examples.

A small Rust CLI that reads and writes the TOML files used by the `/plan-new`, `/implement`, `/plan-update`, `/review`, `/optimise`, `/review-apply`, and `/optimise-apply` commands, plus the repo-scoped capture log behind `/backlog`.

## When to use this skill

Every flow-TOML mutation routes through `tomlctl` — no Python, no line-level `Edit`, no `jq` for TOML parsing. Reach for it whenever a flow command needs to read, filter, or mutate `context.toml`, the review / optimise ledgers, `plan-review-findings.toml`, or their sidecar array-of-tables (`rollback_events`, task-completion records). Shell-level post-processing of tomlctl's JSON output is not needed either — prefer in-tool primitives (`--raw` / `--lines` / `--count-distinct` / `--count`) over piping through `jq -r .count` / `jq -r '.[]'` / `| sort -u | wc -l`.

## Quick Reference

The highest-frequency patterns. Deeper treatment lives in the reference files listed under [References](#references).

| Task | Command |
|---|---|
| Append one item (JSON arg) | `tomlctl items add <file> --json '{...}'` |
| Append one item (stdin) | `cat payload.json \| tomlctl items add <file> --json -` |
| Batch append homogeneous items | `tomlctl items add-many <file> --ndjson <path>` |
| Apply heterogeneous batch (add/update/remove) | `tomlctl items apply <file> --ops -` |
| Filter items | `tomlctl items list <file> --where status=open` |
| Count / bucket items | `tomlctl items list <file> --count` / `--count-by status` / `--group-by file` |
| Next monotonic id | `tomlctl items next-id <file> --prefix R\|O\|E\|P` |
| Bump scalar field | `tomlctl set <file> <key.path> <value>` |
| Set array / sub-table | `tomlctl set-json <file> <key.path> --json '<json>'` |
| Read value via json subcommand | `tomlctl json get <file> <path>` |
| Write value via json subcommand | `tomlctl json set <file> <path> --json <value>` |
| Delete a key at path | `tomlctl json unset <file> <path>` |
| Capture / triage the repo backlog (`.claude/backlog.toml`) | `tomlctl backlog add\|check\|list\|show\|relate\|triage\|cluster\|compact\|evidence {dir\|audit}` |
| Manage active-flow registry | `tomlctl flow active list\|add\|remove\|touch [--slug <s>] [--branch <b>] [--worktree <w>] [--scope <glob>]...` |
| Pre-flight envelope (resolve + doctor + plansDirectory in one dispatch) | `Task(subagent_type: "flow-bootstrap", prompt: <input-envelope-JSON>)` ([see `claude/agents/flow-bootstrap.md`](#flow-bootstrap-agent-entrypoint)) |
| Build the flow-bootstrap input envelope (Step-0 of every flow carrier) | `tomlctl flow envelope build --command <c> [--branch <b>] [--worktree <w>] [--cwd <p>] [--path-arg <p>]... [--require-artifact <a>]...` |
| Run flow invariant checks (optionally auto-fix) | `tomlctl flow doctor [--slug <s>] [--fix]` |
| Report (or bootstrap) flow artifact + sidecar status | `tomlctl flow ensure-artifact --slug <s> --kind <k> [--bootstrap]` |
| Locate plan files | `tomlctl flow find-plans [--dirs <d>...] [--strict-read]` |
| Seed a new flow (context.toml + execution-record.toml + active entry) | `tomlctl flow init --slug <s> --plan <path> [--branch <b>] [--scope <glob>]...` |
| Enumerate flows under .claude/flows/ | `tomlctl flow list [--status <s>] [--branch <b>] [--active-only]` |
| Resolve the active flow (5-step algorithm, emits artifacts + scope) | `tomlctl flow resolve [--flow <s>] [--path <p>]... [--branch <b>] [--worktree <w>] [--with-staleness]` |
| Check whether a flow is stale | `tomlctl flow stale --slug <s> [--threshold <duration>]` |
| Regenerate PROGRESS-LOG.md from the execution record | `tomlctl flow render-progress-log --slug <s> [--stdout] [--verify-integrity]` |
| Refresh integrity sidecar | `tomlctl integrity refresh <file>` |

<a id="flow-bootstrap-agent-entrypoint"></a>**`flow-bootstrap` agent entrypoint**: per-command pre-flight is delegated to the `flow-bootstrap` sub-agent (`claude/agents/flow-bootstrap.md`), which composes `tomlctl flow resolve --with-staleness`, `tomlctl flow doctor`, and (for `plan-new` / `plan-update` / `review-plan`) `tomlctl json get .claude/settings.json plansDirectory` into a single JSON envelope. Each carrier's `## Step 0: Pre-flight (flow resolution + doctor)` section dispatches via `Task` with `subagent_type: "flow-bootstrap"` and a JSON-encoded input envelope; downstream phases consume `envelope.resolved.{slug,context_path,artifacts.*,status,plan_path,scope,stale}` plus `envelope.doctor.ok` instead of running the resolve / doctor primitives inline. The agent is read-only — never passes `--fix` to doctor — so auto-repair stays an orchestrator decision.

## References

The per-verb flag tables, recipes, and contract prose live in four sibling files. Each is self-contained and opens with its own `## Contents` list.

- [references/query.md](references/query.md) — the read-only verbs: `get` / `parse` / `validate`, the full `items list` query surface (filters, projection, shaping, aggregation, output shapes), `items get`, `items find-duplicates`, `items orphans`.
- [references/write.md](references/write.md) — the mutating verbs: `set`, `set-json`, `array-append`, the `items` batch verbs, `integrity refresh`, plus auto-create, `--dry-run`, stdin payload handling, and the dedup fingerprint contract.
- [references/flow.md](references/flow.md) — the cross-cutting surface: the `--verify-integrity` support matrix, what the `.sha256` sidecar does and does not promise, the `--error-format json` envelope, the two emitting `flow` verbs, and the infrastructure-only `blocks` verbs.
- [references/backlog.md](references/backlog.md) — the `backlog` group's flag tables, the `.claude/backlog.toml` store shape, id derivation, the `check` verdict ladder, and the evidence drop-box. When to mint a row is the `backlog-capture` skill's call, not this one's.

To find a section without reading a whole file:

```bash
grep -n '^##' claude/skills/tomlctl/references/<file>.md
```

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

`tomlctl capabilities` emits a stable JSON document (`{"version":"…","features":[…],"subcommands":[…]}`) so downstream templates can feature-gate at boot without parsing `--help` prose. Features are stable within a minor release; new flags add new feature entries rather than being version-qualified. Example invocation (truncated):

```bash
tomlctl capabilities
# {"version":"0.6.0","features":["raw","lines","dedupe_by","dry_run","agent_context",...],"commands":{...}}
```

Representative entries:

| Feature | What it enables |
|---|---|
| `count_distinct` | `--count-distinct <FIELD>` on `items list` |
| `raw` / `lines` | `--raw` / `--lines` output shapes |
| `dedupe_by` / `dedup_id_auto` | `--dedupe-by <FIELDS>` + auto-populate on every write |
| `find_duplicates_across` | `items find-duplicates --across <other>` (tier A/B) |
| `error_format_json` | `--error-format json` + `ErrorKind` taxonomy |
| `strict_read` / `dry_run` | `--strict-read` on reads / `--dry-run` on all 9 write subcommands (`set`, `set-json`, `array-append`, `items add`, `items add-many`, `items update`, `items remove`, `items apply`, `items backfill-dedup-id`) |
| `backfill_dedup_id` / `integrity_refresh` | legacy upgrade + sidecar regen |
| `agent_context` | `tomlctl capabilities` (the `.commands` field of the JSON output) emits a per-subcommand flag schema (type/required/default/values/repeatable + mutex_groups) for runtime introspection without parsing --help prose. |

### Agent-context schema (`tomlctl capabilities` — `.commands` field)

When `features` includes `agent_context`, the capabilities document also carries a `commands` key — a per-subcommand JSON tree that lets agents drive flag assembly programmatically instead of regex-matching `--help` text.

Shape: `commands.<subcommand>` (recursively for nested subcommands like `items.subcommands.list`) carries:

- `flags` — map of flag-name → entry. Each entry has `type` (`string` / `bool` / `enum`), `required` (bool), `repeatable` (bool), and optional `default` and `values` (allowed enum variants). Positional arguments appear with their angle-bracketed display name (e.g. `<file>`).
- `mutex_groups` — list of clap `ArgGroup` mutex sets; an agent can refuse a combination locally without round-tripping through the binary. Note: mutex groups are assembled from two sources — clap's generated `ArgGroup` declarations AND a supplementary `MUTEX_GROUPS` const in `capabilities.rs`. If you extend the CLI, update both; omitting the const supplement will produce incomplete mutex data in the capabilities output.
- `subcommands` — present only when the command has nested subcommands (e.g. `items`, `blocks`, `integrity`).

Feature-gate on its presence before driving flags from the schema:

```
# pseudocode
caps = tomlctl_capabilities()
if "agent_context" in caps.features:
    schema = caps.commands["items"]["subcommands"]["update"]["flags"]
    # build the invocation from schema
else:
    # fallback: parse `tomlctl items update --help`
```

## Constraints and gotchas

- **No comment preservation.** The schemas forbid inline comments, so this is fine for flow/ledger files. Do not point `tomlctl` at TOML files where comments matter.
- **Whole-file rewrite.** Any write operation reparses, mutates, and re-serialises the whole document. Never runs a line-level Edit.
- **Whitespace may change.** Long inline arrays may be reflowed to multi-line by the serializer. Semantically identical.
- **`created` is preserved verbatim.** The tool never touches it unless you explicitly `set created <date>` (don't).
- **`dedup_id` auto-populates on every write** unless `TOMLCTL_NO_DEDUP_ID=1`. First-time upgrade of a legacy item (add/add-many path) populates without marking it as a user-intended change — the sidecar refresh is an implicit one-time event.
- **Unknown-value rules stay with the caller.** `tomlctl` returns raw values; the command's "unknown status → treat as in-progress" / "unknown category → fail-soft" rules apply in the calling command's logic, not in the tool.
- **Errors exit non-zero and print to stderr.** Success paths emit either JSON data (or `--raw` / `--lines` bare text) or `{"ok":true,…}` to stdout. Always check exit code in scripted flows. For machine-readable error class, use `--error-format json`.
- **Lock timeout: 30 seconds.** Writes acquire an exclusive OS-level lock on a hashed lock file under `<repo-top-level>/.claude/.locks/<sha256-of-canonical-target-path>.lock`, rather than a sidecar `<file>.toml.lock` that could collide with a real file of that name. `tomlctl` polls `try_lock_exclusive` on this file and bails after 30 s total with an error naming the lock path. On Windows this is a mandatory lock — a crashed or stuck `tomlctl` leaves the `.lock` file present and the OS keeps the lock until the offending process dies. **Recovery when a lock is stranded:** confirm no live `tomlctl` process holds it (Task Manager / `Get-Process tomlctl` / `ps aux | grep tomlctl`), then delete the specific `.claude/.locks/<hash>.lock` file from the error message. The next invocation will recreate and re-acquire it cleanly.
- **Write-path safety (best-effort containment guard, not a sandbox).** Write operations (`set`, `set-json`, `items add|update|remove|apply|add-many|backfill-dedup-id`, `array-append`) reject targets that canonicalise outside the current repo's `.claude/` directory. The guard resolves symlinks and `..` at canonicalisation time and rejects paths not under `<git-top-level>/.claude/`. Read operations are not guarded. Pass `--allow-outside` (a per-subcommand flag) to override when you genuinely need to edit a flow TOML elsewhere — e.g. `tomlctl set /tmp/scratch.toml status draft --allow-outside`. `--allow-outside` is pinned behind an interactive permission prompt at the project settings level — it should never appear in unattended automation. Treat this as a best-effort guard against agent/user typos that would otherwise land writes in unintended locations; it is not a security sandbox and a TOCTOU-race or symlink swap between canonicalisation and open can in principle escape it. Note the interaction with auto-create: with `--allow-outside` disabling the guard AND auto-create on by default, a path typo can silently create a stray file anywhere on disk — a deliberate double opt-out. Pair `--allow-outside` with `--no-create` whenever the target should already exist.

## Permissions

`Bash(tomlctl *)` is pre-approved in the project's `.claude/settings.json`. Any invocation passing `--allow-outside` is explicitly denied by three deny rules in that same file and falls through to an interactive permission prompt:

```json
"deny": [
  "Bash(tomlctl --allow-outside *)",
  "Bash(tomlctl * --allow-outside)",
  "Bash(tomlctl * --allow-outside *)"
]
```

Agents should never emit `--allow-outside` unattended — the write-path containment guard is default-on for a reason.
