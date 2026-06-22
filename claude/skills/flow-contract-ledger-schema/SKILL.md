---
name: flow-contract-ledger-schema
description: Canonical schema for review/optimise ledgers — `[[items]]` table shape, required and optional fields, status vocabulary, vet-event log, dedupe contract, and write idioms for `review-ledger.toml` and `optimise-findings.toml` (both flow-local and flow-less `.claude/reviews/<scope>.toml` / `.claude/optimise-findings/<scope>.toml`). Canonical source for this schema, with identical copies still embedded in the optimise/review-apply/optimise-apply carriers pending migration; defines every field a finding must carry plus the reconciler's behaviour on schema drift. Consult before any read, write, or update against a review or optimise ledger TOML file.
---

## Ledger Schema

All `.claude/...` ledger paths below — whether flow-local (`review-ledger.toml`, `optimise-findings.toml`) or flow-less (`.claude/reviews/<scope>.toml`, `.claude/optimise-findings/<scope>.toml`) — share the single canonical schema defined in this section. This skill is the canonical source for the schema; identical copies remain embedded verbatim in `review-apply.md`, `optimise.md`, and `optimise-apply.md` pending their migration to reference this skill, so every command that reads or writes a ledger sees the same rules. Read this section before touching any ledger read/write logic.

### Canonical Ledger Schema (single source of truth)

Both `review-ledger.toml` and `optimise-findings.toml` share this schema. Required fields marked — others optional. No inline comments in emitted TOML.

```toml
schema_version = 1
last_updated = 2026-04-16

[[items]]
id = "R1"
file = "src/accounting/postings.rs"
line = 66
severity = "critical"
effort = "small"
category = "quality"
summary = "Trade sell wrong journal entries"
first_flagged = 2026-04-08
rounds = 1
status = "fixed"
resolved = 2026-04-08
resolution = "Gated with BooksError in ca44570"
flow = "warm-meandering-zebra"

[[items]]
id = "R22"
file = "src/events/listeners.rs"
line = 84
symbol = "listeners::trigger"
severity = "suggestion"
effort = "small"
category = "architecture"
summary = "Listeners bypass pipeline API, call deriver directly"
description = "Re-entrancy risk: pipeline mutex could deadlock if listeners call pipeline service."
first_flagged = 2026-04-08
rounds = 1
status = "deferred"
defer_reason = "Architectural change with re-entrancy risk"
defer_trigger = "When pipeline mutex is replaced with a channel-based design"
related = []
```

#### Required fields (every item)

- `id` — `R{n}` for review items, `O{n}` for optimise items. Stable; never renumbered; monotonic per-ledger.
- `file` — repo-relative file path.
- `line` — integer. Use `0` if no specific line applies.
- `severity` — `critical` | `warning` | `suggestion`.
- `effort` — `trivial` | `small` | `medium`.
- `category` — see vocabulary below.
- `summary` — one-line description.
- `first_flagged` — TOML date, ISO 8601.
- `rounds` — integer, incremented each time the same issue is re-flagged in a later run.
- `status` — see disposition vocabulary below.

#### Optional fields

- `symbol` — function / struct / trait method name. **Strongly recommended** for line-drift resilience; omit if no natural anchor applies.
- `description` — longer explanation when `summary` is insufficient.
- `evidence` — array of strings: doc URLs, Context7 query citations, benchmark links.
- `related` — array of peer IDs (e.g. `["R5", "R8"]`).
- `flow` — slug of the flow that contains or resolved this item. Empty/omitted for flow-less ledgers.
- `depends_on` — array of ledger IDs (e.g. `["O7", "R12"]`) this item must apply AFTER. Consumed by the topological sort in `/review-apply` and `/optimise-apply` Step 3. Forward references to non-existent IDs are harmless — the topo sort restricts the DAG to the selected set — but `tomlctl items orphans <ledger>` surfaces dangling refs for hygiene (emits `{"id":...,"class":"dangling-dep","dangling_deps":[...]}` records alongside `missing-file` and `symbol-missing` classes).
- `fingerprint` — opaque string computed by `tomlctl` (not hand-authored). Produced by `tomlctl items find-duplicates --tier B` as a 16-char SHA-256 truncation over `file|summary|severity|category|symbol`; current ledgers leave this field absent. Consumers treat absence as "fingerprint not yet computed".
- `rollback_rationale` — string; present on items whose transition was reverted by a Step 5.5 rollback in `/review-apply` or `/optimise-apply`. Set when a rollback flips an item from `fixed`/`applied` back to `open`. Preserved across subsequent rounds so the rollback history surfaces in future reports.
- `reopen_rationale` — string; present on items whose status was transitioned from `deferred` back to `open` via the deferred-trigger reopen sweep (`/review` and `/optimise` Step 1). Captures the trigger event that fired.

#### Disposition-specific fields (required only when status matches)

- `status = "fixed"` / `status = "applied"`:
  - `resolved` (date, required)
  - `resolution` (string, required) — commit SHA + short description.
- `status = "deferred"`:
  - `defer_reason` (string, required)
  - `defer_trigger` (string, required) — concrete re-evaluation condition.
- `status = "wontfix"` / `status = "wontapply"`:
  - `wontfix_rationale` (string, required).
- `status = "verified-clean"`:
  - `verified_note` (string, required) — the audit note (e.g. "Round 2 (2026-04-14) — migrations.rs idioms already match").

#### Category vocabularies

- **Review** (`review-ledger.toml`): `quality` | `security` | `architecture` | `completeness` | `db` | `testability` | `package-quality` | `verified-clean` (reserved for items with `status = "verified-clean"`).
- **Optimise** (`optimise-findings.toml`): `memory` | `serialization` | `query` | `algorithm` | `concurrency`.

**Unknown-value fail-soft rules** (mandatory):
- Unknown `status` → treat as `open`.
- Unknown `category` → treat as `quality` (review) or `memory` (optimise); write a one-line warning to the command's console output but do not error.

#### Disposition vocabulary

- `open` — active, needs resolution.
- `deferred` — not acting now, with a concrete re-eval trigger.
- `fixed` (review) / `applied` (optimise) — resolved with commit evidence.
- `wontfix` (review) / `wontapply` (optimise) — intentional non-action with rationale.
- `verified-clean` (review only) — explicitly audited and confirmed clean; kept to avoid re-flagging via dedup. `/optimise` has no `verified-clean` counterpart — bytes-written findings land in `applied`, already-correct cases land in `wontapply` with rationale.

#### Render-to-markdown contract

Commands emit TOML as the authoritative artifact. For human-readable console output, commands render items as grouped markdown tables (severity-grouped for new-finding reports; disposition-grouped for full ledger views) inline in their response. The rendered markdown is not persisted.

#### Rollback event log

When `/review-apply` or `/optimise-apply` Step 5.5 reverts a batch of transitions, the protocol appends one `[[rollback_events]]` table to the ledger root:

```toml
[[rollback_events]]
timestamp = 2026-04-17T14:32:00Z
command = "review-apply"
cause = "build failure on src/accounting/postings.rs:122"
items = ["R3", "R7"]
stash_ref = "stash@{0}"
```

Fields:
- `timestamp` — ISO 8601 date-time (seconds precision).
- `command` — `"review-apply"` or `"optimise-apply"`.
- `cause` — short description (build fail, test regression, or claimed-applied-without-diff).
- `items` — array of ledger IDs that were reverted back to `status = "open"`.
- `stash_ref` — `git stash` reference for the rolled-back working-tree state so the user can recover the changes if desired.

`[[rollback_events]]` is append-only; existing entries are never rewritten or deleted. If the log grows unwieldy, older entries may be archived manually by moving them to `<ledger>.rollback-history.toml`; no command automates this yet.

#### Vet event log

When `/review`, `/optimise`, `/review-plan`, `/plan-new`, `/plan-update`, or `/test-bootstrap` runs the vet-research procedure (see the `flow-contract-vet-research` skill), the orchestrator appends one `[[vet_events]]` table per vetted agent to the ledger root:

```toml
[[vet_events]]
timestamp = 2026-05-08T14:32:00Z
command = "review"
agent_index = 2
lens = "security"
sampled_count = 5
dropped_count = 1
downgraded_count = 0
dropped_ids = ["R47"]
rationale = "R47 cited file:line that does not exist on disk"
```

Fields:
- `timestamp` — ISO 8601 date-time (seconds precision).
- `command` — one of `"review"`, `"optimise"`, `"review-plan"`, `"plan-new"`, `"plan-update"`, `"test-bootstrap"`.
- `agent_index` — integer 1..N matching the `Agent-{n}` index in the mandatory console line emitted by step 6 of the `vet-research` block.
- `lens` — string. The lens name as printed in the console line (e.g. `"security"`, `"test-runner"`, `"coverage"`).
- `sampled_count`, `dropped_count`, `downgraded_count` — integers matching the N / M / K values in the console line.
- `dropped_ids` — array of `R{n}` / `O{n}` ledger IDs that were vetted-out (dropped or downgraded). Empty array when nothing was dropped.
- `rationale` — string capped at 8 KiB per the field-length-cap convention. Multi-line allowed (TOML multi-line strings).

`[[vet_events]]` is append-only; existing entries are never rewritten or deleted. If the log grows unwieldy, older entries may be archived manually by moving them to `<ledger>.vet-history.toml`; no command automates this yet. Distinguish from `[[rollback_events]]` (which captures Step 5.5 working-tree reverts, not Step 2.5 / Phase 1.5 / Phase 2.5 / Phase 3 vet-pass actions).

### Ledger TOML read/write contract

Applies to every read/write of `review-ledger.toml` and `optimise-findings.toml`. This contract is DIFFERENT from the `context.toml` contract (single-object file, line-edit-safe) because ledgers use arrays-of-tables which are fragile under line-based editing (two items with identical `status = "open"` / `rounds = 1` lines defeat the Edit tool uniqueness).

#### Read rules

- **Missing `schema_version`**: treat as `1` and write it back on the next write. This is the only silent-default allowed.
- **`schema_version > 1`**: halt and ask the user — we don't know this format.
- **Missing required item field**: flag the item in the console output as malformed, skip it for resolution / dedup; do NOT attempt auto-repair.
- **TOML parse error**: report the error location; do NOT attempt auto-repair; ask the user to fix or restore from backup.

#### Write strategy (MANDATORY)

**Ledger writes MUST use parse-rewrite, not line-edit.** Preferred path — `tomlctl` (see skill `tomlctl`):

**First write auto-creates the ledger.** Every mutating verb below routes through `tomlctl`'s `mutate_doc*` chokepoint, so a FIRST write against a missing ledger creates it on demand, seeded with the schema-aware skeleton (`schema_version = 1` + `last_updated = <today>`) — byte-identical to `flow init`'s bootstrap. This holds for all four ledger paths this skill covers: the flow-local `review-ledger.toml` / `optimise-findings.toml` and the flow-less `.claude/reviews/<scope>.toml` / `.claude/optimise-findings/<scope>.toml`. Callers therefore do NOT need to initialise the file first — a bare `items add` (or any `mutate_doc*` verb) against a non-existent ledger is sufficient; the write envelope reports `"created": true` and one stderr line confirms the seed. (A no-match `update`/`remove` against a freshly-seeded doc still errors and leaves no file. Pass `--no-create` to restore the strict prior behaviour where a missing file yields `kind=not_found`.)

- `tomlctl items add <ledger> --json '{...}'` — append a new item.
- `tomlctl items update <ledger> <id> --json '{...}'` — patch fields on an existing item matched by `id`.
- `tomlctl items remove <ledger> <id>` — delete by id.
- `tomlctl items apply <ledger> --ops -` (stdin heredoc — preferred) or `tomlctl items apply <ledger> --ops '[{"op":"add|update|remove", ...}, ...]'` (argv; small fixed-string batches only) — batch multiple **heterogeneous** ops (mixed add/update/remove, or non-uniform field sets) in one atomic, all-or-nothing file rewrite. Use this whenever touching several items in the same run so the ledger pays one parse + one write instead of N. Feed the ops array via heredoc — the same `<<'EOF' … EOF` pattern as the `add-many` example below, except the payload is a JSON array of op objects piped into `--ops -` instead of NDJSON. Never stage the ops payload via a tempfile; the `-` sentinel is the agent-native replacement for that round-trip.
- `tomlctl items add-many <ledger> --ndjson - [--defaults-json '{...}']` — batch-append **homogeneous** new items via newline-delimited JSON on stdin; shared fields go in `--defaults-json` and per-row keys win. Prefer this over a hand-rolled `--ops` array when every op is `"add"`. Example:
  ```bash
  tomlctl items add-many <ledger> \
    --defaults-json '{"first_flagged":"2026-04-18","rounds":1,"status":"open"}' \
    --ndjson - <<'EOF'
  {"id":"R40","file":"src/a.rs","line":10,"severity":"warning","effort":"small","category":"quality","summary":"..."}
  {"id":"R41","file":"src/b.rs","line":22,"severity":"suggestion","effort":"trivial","category":"quality","summary":"..."}
  EOF
  ```
- `tomlctl array-append <ledger> <array-name> --json '{...}'` (or `--ndjson -` for many) — append to an append-only array-of-tables (e.g. `rollback_events`) without op-type JSON framing. Thin shim over `items apply --array <name>`; use this for readable single-entry appends.
- `tomlctl set <ledger> last_updated <YYYY-MM-DD>` — bump the file-level `last_updated`.
- `tomlctl items next-id <ledger> --prefix R|O` — compute the next monotonic id.
- **Reads / queries** — `tomlctl items list <ledger>` carries a full query surface; reach for it instead of piping `tomlctl parse` through another language:
  - `--status open --count` — gate count (emits `{"count": N}`).
  - `--group-by file --select id,symbol` — regression-style grouping (emits `{"<file>":[{id, symbol}, ...], ...}`).
  - `--count-by status` — disposition histogram.
  - `--pluck id` — flat `["R1","R2",...]` list.
  - `--where KEY=VAL`, `--where-in KEY=V1,V2`, `--where-has KEY`, `--where-gte KEY=@date:YYYY-MM-DD`, `--where-regex KEY=PAT` — filter composition. Typed RHS via `@date:` / `@int:` / `@float:` / `@bool:` prefixes; bare strings otherwise.
- **Stdin for `--ops` / `--json` / `--ndjson`**: every JSON-accepting flag above treats `-` as a sentinel meaning "read JSON from stdin" — e.g. `printf '%s' "$OPS" | tomlctl items apply <ledger> --ops -`. Prefer this for large batches or payloads containing shell metacharacters (embedded quotes, `$`, backticks, or newlines in agent-produced `resolution` / `wontfix_rationale` / `verified_note` strings); avoids the tempfile round-trip and eliminates the argv-level quoting surface entirely. Empty stdin errors clearly.

`tomlctl` writes go through `tempfile::NamedTempFile::persist` (atomic rename) and hold an exclusive advisory lock on a sidecar `.lock` file, so concurrent invocations are safe and an interrupted write cannot corrupt the ledger.

If `tomlctl` is unavailable, install it: `cargo install --path tomlctl`.

#### Key-order convention (for serialisers that don't preserve order)

When re-serialising an item, emit keys in this order:
`id, file, line, symbol, severity, effort, category, summary, description, evidence, first_flagged, rounds, related, status, <disposition-specific fields>, flow`

The file-level keys come first: `schema_version`, `last_updated`, then `[[items]]` entries. `schema_version` MUST be preserved on every write.

### Item-ID assignment and dedup

- **ID assignment**: R-numbers for review items, O-numbers for optimise items. New items get `max(existing) + 1`. Never renumber. IDs retired by deletion are never reused.
- **Dedup rule (same for new-item merge AND regression detection)**: two findings match iff they have the **same `file`** AND (**same non-empty `symbol`** OR **exact `summary` string match**). No fuzzy matching, no keyword clustering. When in doubt, new ID.
- **Merge behaviour**:
  - New finding matches an `open` item → reuse the existing ID; increment `rounds`; update `last_updated` of the ledger.
  - New finding matches a `fixed` / `applied` item → **regression**; assign a new ID; write `related = ["<old id>"]`; flag prominently in the console report.
  - New finding matches a `deferred` / `wontfix` / `wontapply` / `verified-clean` item → treat as existing (no change); do not emit a new item; do not increment `rounds`. Note in console: "this matches an existing <status> item, not re-reporting."
- **Chronic-item escalation**: `rounds >= 3` on `open` items escalates in the summary output.
