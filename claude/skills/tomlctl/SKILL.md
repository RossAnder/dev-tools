---
name: tomlctl
description: Read and write TOML files used by Claude Code flows — `.claude/flows/*/context.toml`, `review-ledger.toml`, `optimise-findings.toml`. Use this skill instead of `python3 -c "import tomllib"` whenever parsing, querying, or mutating these files. Works on Windows and Linux; outputs JSON for easy consumption.
---

# tomlctl

A small Rust CLI that reads and writes the TOML files used by the `/plan-new`, `/implement`, `/plan-update`, `/review`, `/optimise`, and `/optimise-apply` commands. It replaces the `python3 -c "import tomllib"` approach that breaks on Windows Git Bash.

## When to use this skill

Use `tomlctl` whenever a flow command needs to:
- Resolve a flow's `scope`, `branch`, `status`, or `artifacts.*` from `context.toml`.
- Read or filter `[[items]]` in `review-ledger.toml` / `optimise-findings.toml`.
- Update a single scalar (`status`, `updated`, `tasks.completed`) in `context.toml`.
- Append a new `[[items]]` entry, or patch fields on an existing item by `id`.
- Compute the next `R{n}` / `O{n}` id without loading Python.

If the binary isn't on PATH, skip this skill and fall back to the `python3` parse-rewrite path described in each command's TOML read/write contract.

## Install

One-time, per machine:

```powershell
# from the dev-tools repo root
cargo install --path tomlctl
```

That drops `tomlctl` into `~/.cargo/bin/` (already on PATH if Rust is installed). Verify:

```powershell
tomlctl --version
```

On Linux/macOS the same `cargo install --path tomlctl` works.

## Read operations

All read commands print JSON on stdout. Pipe through `Select-Object` / `jq` / `ConvertFrom-Json` as needed.

```bash
# Whole document as JSON
tomlctl parse .claude/flows/auth-overhaul/context.toml

# Single value at a dotted path
tomlctl get .claude/flows/auth-overhaul/context.toml status
tomlctl get .claude/flows/auth-overhaul/context.toml tasks.completed
tomlctl get .claude/flows/auth-overhaul/context.toml artifacts.optimise_findings

# Whole [[items]] array (JSON array of objects)
tomlctl items list .claude/flows/auth-overhaul/review-ledger.toml

# Filter items by status
tomlctl items list .claude/flows/auth-overhaul/review-ledger.toml --status open

# Fetch one item
tomlctl items get .claude/flows/auth-overhaul/review-ledger.toml R22

# Parse-check (exit 0 on valid)
tomlctl validate .claude/flows/auth-overhaul/context.toml
```

TOML dates render as ISO-8601 strings in the JSON output.

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

### Append a new item

`--json` takes a single JSON object representing the new `[[items]]` entry. Field order in the JSON becomes field order in the emitted TOML, so pass fields in the canonical key order from `## Ledger Schema`:
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

Date-shaped strings (`YYYY-MM-DD`) are automatically promoted to TOML date literals.

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

### Batch multiple item ops (`items apply`)

For runs that touch several `[[items]]` in the same ledger, use `items apply` to parse + rewrite the file once. `--ops` is a JSON array; each op is `{"op": "add|update|remove", ...}` with the same payload shape as the single-op commands (`json` for add/update, `id` for update/remove). Ops run in array order; any op error aborts the whole batch and the file is left unchanged.

```bash
tomlctl items apply .claude/flows/foo/review-ledger.toml --ops '[
  {"op":"add",    "json":{"id":"R24","severity":"minor","summary":"...","status":"open"}},
  {"op":"update", "id":"R22", "json":{"status":"applied","resolved":"2026-04-17"}},
  {"op":"remove", "id":"R17"}
]'
```

Prefer this over looping single-op invocations — one parse + one write instead of N.

### Stdin input for large JSON payloads

All JSON-accepting flags (`--ops`, `--json` on `items add` / `items update` / `set-json`) treat the literal value `-` as "read JSON from stdin". Use this to avoid shell-quoting or tempfile round-trips when the JSON payload is large (batches of 10+ items, or values containing quotes / newlines / dollar signs):

```bash
# bash — pipe via process substitution or heredoc
printf '%s' "$OPS_JSON" | tomlctl items apply ledger.toml --ops -

tomlctl items apply ledger.toml --ops - <<'EOF'
[{"op":"update","id":"R1","json":{"status":"fixed"}}]
EOF
```

```powershell
# PowerShell — pipe a string or file
$ops | tomlctl items apply ledger.toml --ops -
Get-Content ops.json | tomlctl items apply ledger.toml --ops -
```

Empty stdin is an error. Use the literal CLI-argument form for small payloads; use stdin for anything that would require complex shell escaping.

## Constraints and gotchas

- **No comment preservation.** The schemas forbid inline comments, so this is fine for flow/ledger files. Do not point `tomlctl` at TOML files where comments matter.
- **Whole-file rewrite.** Any write operation reparses, mutates, and re-serialises the whole document — identical to the Python parse-rewrite strategy the commands mandate. Never runs a line-level Edit.
- **Whitespace may change.** Long inline arrays may be reflowed to multi-line by the serializer. Semantically identical.
- **`created` is preserved verbatim.** The tool never touches it unless you explicitly `set created <date>` (don't).
- **Unknown-value rules stay with the caller.** `tomlctl` returns raw values; the command's "unknown status → treat as in-progress" / "unknown category → fail-soft" rules apply in the calling command's logic, not in the tool.
- **Errors exit non-zero and print to stderr.** Success paths emit either JSON data or `{"ok":true}` to stdout. Always check exit code in scripted flows.
- **Lock timeout: 30 seconds.** Writes acquire an exclusive OS-level lock on a sidecar `.lock` file next to the target (e.g. `review-ledger.toml.lock`). `tomlctl` polls `try_lock_exclusive` every 500 ms and bails after 30 s total with an error naming the lock path. On Windows this is a mandatory lock — a crashed or stuck `tomlctl` leaves the `.lock` file present and the OS keeps the lock until the offending process dies. **Recovery when a lock is stranded:** confirm no live `tomlctl` process holds it (Task Manager / `Get-Process tomlctl` / `ps aux | grep tomlctl`), then delete the `<target>.lock` file. The next invocation will recreate and re-acquire it cleanly.
- **Write-path safety.** Write operations (`set`, `set-json`, `items add|update|remove|apply`) refuse to touch files outside the current repo's `.claude/` directory. The guard canonicalises the target (resolving symlinks and `..`) and rejects paths not under `<git-top-level>/.claude/`. Read operations are not guarded. Pass `--allow-outside` (a global flag, before the subcommand) to override when you genuinely need to edit a flow TOML elsewhere — e.g. `tomlctl --allow-outside set /tmp/scratch.toml status draft`. This protects against agent-influenced `artifacts.*` paths pointing at credential or config files outside the flow tree.

## Fallback

If `tomlctl` isn't available (missing binary, cargo not installed, etc.), fall back to the `python3 -c "import tomllib"` parse-rewrite strategy documented in each command's `## Ledger Schema` and `### TOML read/write contract` sections. Do not attempt line-level Edits on ledger files with multiple `[[items]]` entries — the uniqueness-of-match constraint is not satisfiable.
