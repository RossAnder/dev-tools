# Plan: Apply Trevin Chow's "Agent-Native CLI" principles to tomlctl

**Plan path**: `.claude/plans/glistening-wiggling-valiant.md`
**Created**: 2026-05-08
**Status**: Draft
**Source article**: https://trevinsays.com/p/10-principles-for-agent-native-clis (Trevin Chow, May 2026)

## Context

The article enumerates ten principles for CLIs designed primarily for agent consumers. Mapping the ten principles against tomlctl's current surface (Cargo.toml v0.3.0, `tomlctl/src/`, `claude/skills/tomlctl/SKILL.md`) gives:

| # | Principle | tomlctl status |
|---|---|---|
| 1 | Non-interactive | ✅ already |
| 2 | Structured output | ✅ JSON default + `--error-format json` |
| 3 | Errors that enumerate | ⚠ ~17% at tier-C, ~56% zero-enum |
| 4 | Safe retries / explicit mutation | ⚠ `--dry-run` on 3 of 9 write subcommands |
| 5 | Bounded responses | N/A — local file tool |
| 6 | Cross-CLI vocabulary | Marginal — rename churn outweighs benefit |
| 7 | Three-layer introspection | ⚠ `capabilities` is feature-tags only, no flag schema |
| 8 | Async-aware | N/A — synchronous parse-rewrite |
| 9 | Profiles | N/A — file path is the identity |
| 10 | Two-way I/O | Marginal — no `--deliver`/`feedback` fit a write-to-file tool |

Three changes are worth the work: extend `capabilities` into a true `agent-context` (Principle 7), bring `--dry-run` to all write subcommands (Principle 4), and rewrite zero-enumeration error sites (Principle 3). Principles 5/6/8/9/10 are explicitly out of scope (rationale below).

## Scope

- **In scope**:
  1. Extend `tomlctl capabilities` JSON to emit a per-subcommand flag schema (additive — keep `version`/`features`/`subcommands` keys unchanged).
  2. Add `--dry-run` to `items add` / `items add-many` / `items update` / `set` / `set-json` / `array-append`.
  3. Rewrite zero-enumeration error sites identified in the Phase 1 audit (~29 high-priority sites from the 84-site survey).
  4. Doc updates: `claude/skills/tomlctl/SKILL.md`, `tomlctl/README.md`.
  5. Cargo.toml minor version bump 0.3.0 → 0.4.0.
- **Out of scope** (with justification — keep this list documented; don't expand without conscious review):
  - **Principle 5 (bounded-response truncation hints)** — tomlctl is a local-disk tool, not network. Agents already paginate via `--limit`/`--offset`/`--count`/`--pluck`/`--ndjson`. Adding a `truncated:true,hint:"..."` envelope to every read would noise up output without buying agent benefit.
  - **Principle 6 (rename `add` → `create`, `remove` → `delete`)** — would churn every flow-command invocation in `claude/commands/*.md` and every example in SKILL.md. Aliases would multiply forms without picking a canonical, undermining the principle. Reject.
  - **Principle 8 (async-aware `--wait` / job ledger)** — N/A. All tomlctl operations are synchronous parse-rewrite.
  - **Principle 9 (profiles)** — N/A. The file path *is* the identity for tomlctl, and flow commands always pass it explicitly. No profile concept fits.
  - **Principle 10 (`--deliver` / `feedback`)** — `--deliver` doesn't fit a tool whose write target is the named file argument. `feedback` is overkill for a single-user CLI.
- **Affected areas**:
  - `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs`
  - `tomlctl/src/items.rs`, `tomlctl/src/io.rs`, `tomlctl/src/output.rs`
  - `tomlctl/src/query.rs`, `tomlctl/src/dedup.rs`, `tomlctl/src/convert.rs`, `tomlctl/src/blocks.rs`
  - **NEW**: `tomlctl/src/capabilities.rs` (clap-reflection-driven schema emitter)
  - `tomlctl/tests/capabilities.rs`, `tomlctl/tests/items_dry_run.rs`
  - `tomlctl/Cargo.toml`, `tomlctl/README.md`, `claude/skills/tomlctl/SKILL.md`
- **Estimated file count**: 13 unique files (within the 15-file scope cap).

## Exploration Notes

### Clap surface (Agent 1, file paths abridged)

- `cli/types.rs`: top-level `Cli` (single `error_format` global flag + subcommand dispatch). 10 `Cmd` variants. `ItemsOp` carries 11 sub-variants. `BlocksOp` carries 1. `IntegrityOp` carries 1. Total: ~23 subcommands.
- Shared arg bundles: `ReadIntegrityArgs` (2 bools), `WriteIntegrityArgs` (4 bools), `QueryArgs` (~27 flags incl. 14 repeatable `--where-*` predicates).
- Hand-maintained metadata: `FEATURES` const (13 strings) + `SUBCOMMANDS` const (10 strings) at `cli/types.rs` lines 22–55.
- Capabilities emit at `cli/dispatch.rs:446-460` produces `{version, features, subcommands}` from `env!("CARGO_PKG_VERSION")` + the two const arrays.
- **No runtime clap reflection used today**. Schema info is hand-maintained.

### Existing dry-run plumbing (Agent 2)

- Three pure compute helpers in `items.rs`: `compute_apply_mutation` (line 1100), `compute_remove_mutation` (line 1165), `compute_backfill_mutation` (line 1212). All return `MutationPlan { added, updated, removed, new_doc }` (struct at items.rs:1054).
- Output emitter: `output.rs:38` `emit_dry_run_plan(plan: &MutationPlan)` produces `{"ok":true,"dry_run":true,"would_change":{added:N,updated:M,removed:K,ids:[...]}}`. `backfill-dedup-id` deviates with `{would_backfill:N,ids:[...]}`.
- Dispatch threading: each arm at `cli/dispatch.rs:683/714/895` branches on `dry_run` — dry-run path calls `compute_*` then `emit_dry_run_plan`; live path calls `mutate_doc_plan` (which itself calls the same `compute_*` then writes).
- Compute-vs-write byte-equivalence asserted by tests `compute_apply_mutation_new_doc_matches_live_apply_bytes` (items.rs:2829) and `compute_remove_mutation_matches_live_remove_bytes` (items.rs:2886).
- `items_add` / `items_add_to` / `items_update` / `items_add_many` / `array_append` (items.rs:228 / 233 / 415 / 1005 / 1028) are direct `&mut TomlValue` mutators — no compute helper exists for them yet. **The earlier explore-agent claim of an existing `compute_add_many_outcome` was wrong** — only the `AddManyOutcome` return type is imported in dispatch.rs:33; no compute helper is defined.
- `set` / `set-json` use `mutate_doc(file, allow_outside, opts, |doc| { ... })` (io.rs:325–356) — closure signature already isolates the mutate stage from the I/O wrapper, so `--dry-run` is straightforward: run the closure on a clone, never call the outer `mutate_doc`.

### Error audit (Agent 3)

84 error sites surveyed across `cli/dispatch.rs`, `items.rs`, `query.rs`, `dedup.rs`, `convert.rs`, `blocks.rs`, `io.rs`. Coverage:

- **Tier-C quality (template)**: 14 sites (17%). Best example: `dedup.rs:76` `"tier C is file-scoped; use --tier A or --tier B with --across"` — names what's wrong and what to do.
- **Partial enumeration**: 23 sites (27%). Examples: `"--json must be a JSON object"` (says expected shape but not actual / not example).
- **Zero enumeration**: 47 sites (56%) — primary audit target. Examples:
  - `items.rs:688/815` `"unknown op `{}`"` — should name `add|update|remove`.
  - `items.rs:600/606/630/634/652/780/786/793/797/811` `"... missing `<field>` field"` — should enumerate required op shape.
  - `items.rs:198/224/642/737/740/742/830` `"no item with id = {}"` — could suggest `tomlctl items list <file> --pluck id` to discover.

The full audit ledger (file:line:current_message:class) was produced by Agent 3 and is the input to the Phase-2 rewrite task.

## Research Notes

No external research required for this plan — the work is internal refactor + doc updates against an established Rust codebase. The clap-derive runtime reflection API used in Phase 2 Task 2 is documented in `clap` crate's own docs (in-tree, no Context7 fetch needed):
- `clap::CommandFactory::command()` returns the constructed `clap::Command` from any `#[derive(Parser)]` type.
- `clap::Command::get_subcommands()` walks subcommand tree.
- `clap::Command::get_arguments()` walks args of a single command.
- `clap::Arg::get_id() / .get_action() / .get_value_parser() / .is_required_set() / .get_default_values()` exposes per-flag metadata.
- `clap::builder::PossibleValuesParser` and `clap::builder::ValueParser::possible_values()` exposes enum value sets when a flag uses `#[arg(value_enum)]` against a `ValueEnum` impl.
- `clap::Command::get_groups()` walks `ArgGroup` mutex declarations (e.g. the `items list` shape group).

No `Cargo.toml` feature changes needed — these APIs are available with the existing `clap = { version = "^4", features = ["std","derive","help","usage"] }` configuration.

## User Decisions

**Q: Error-rewrite scope for Task 5 + Task 6(c) — focused, comprehensive, or skip?**
- **A: Comprehensive (~52 sites).** Rewrite the 47 zero-enumeration sites AND the 23 partial-enumeration sites identified in the Phase 1 audit ledger. Skip only the Tier-3 IO pass-through errors that carry inherited `anyhow` chain messages.
- **Implication**: Task 5 and Task 6(c) widen to cover the partial-enum sites. The 23 partial-enum rewrites are mostly type-coercion messages ("--json must be a JSON object") that need to additionally quote the actual received type (e.g. → "--json must be a JSON object (e.g. {\\"key\\":\\"value\\"}); got JSON array"). Lower-leverage than the zero-enum sites but mechanical and finite.
- **Prompted by**: Agent 3's audit summary identifying 56% of error sites at zero-enumeration and 27% at partial — both classes are agent-correction-blocking; the user prefers to close both at once rather than ship two passes.

## Approach

### Three changes, three independent thrusts

#### Change 1 — `capabilities` → richer agent-context (Principle 7)

**Decision**: extend the existing `capabilities` subcommand in-place (additive), do NOT add a new `agent-context` sibling subcommand. Rationale:
- The existing stability contract on `capabilities` (README.md:175-220) already promises additive growth.
- Avoids introducing a sibling subcommand whose only difference would be richer output.
- The article's `agent-context` example *describes* the shape, not the verb name.

**Implementation strategy**: walk clap's runtime command tree via `<Cli as CommandFactory>::command()`. New module `tomlctl/src/capabilities.rs` exposing `pub(crate) fn build_agent_context() -> serde_json::Value` that recursively walks `cmd.get_subcommands()`. The dispatch arm at `cli/dispatch.rs:446` merges its output under a new top-level `commands` key:

```json
{
  "version": "0.4.0",
  "features": [..., "agent_context", ...],
  "subcommands": [...],
  "commands": {
    "items": {
      "subcommands": {
        "list": {
          "flags": {
            "--count": {"type": "bool", "required": false, "default": false},
            "--tier": {"type": "enum", "required": false, "values": ["A","B","C"]},
            "--where": {"type": "string", "required": false, "repeatable": true},
            ...
          },
          "mutex_groups": [["count","count_by","group_by","pluck","count_distinct"]]
        },
        ...
      }
    },
    ...
  }
}
```

Hand-maintained supplement: `MUTEX_GROUPS` const (in `capabilities.rs`, NOT `types.rs`) for semantic constraints clap doesn't expose via `Command::get_groups()` (e.g. `--count-by --raw` rejection in `query.rs`).

Add `"agent_context"` to the `FEATURES` const so callers can feature-gate the new key.

#### Change 2 — `--dry-run` parity across write surface (Principle 4)

**Pattern**: every write subcommand gets a `dry_run: bool` clap field; dispatch arm branches on `dry_run`; new compute helper produces a `MutationPlan` (or scalar variant for set/set-json); `emit_dry_run_plan` (existing) or `emit_dry_run_scalar` (new) prints the JSON envelope.

**New compute helpers** (all in `items.rs` unless noted):
- `compute_add_mutation(doc, array_name, json) -> Result<MutationPlan>` — clones doc, calls `items_add_value_to`, captures id from result.
- `compute_add_many_mutation(doc, array_name, rows, defaults, dedupe_fields) -> Result<MutationPlan>` — when `dedupe_fields` is empty, calls `items_add_many` (returns `Result<usize>`); otherwise calls `items_add_many_with_dedupe` (returns `Result<AddManyOutcome>`). Translates either return into `MutationPlan`: skipped rows from the dedupe path go into the `MutationPlan.skipped` field (already declared at `items.rs:1066` with `#[allow(dead_code)]`, reserved for exactly this purpose). The `defaults: Option<&JsonValue>` parameter is required because `items add-many` accepts `--defaults-json` — omitting it makes the dry-run preview diverge from the live path.
- `compute_update_mutation(doc, array_name, id, json, unset) -> Result<MutationPlan>` — clones doc, calls `items_update_value_to`, populates `updated` with the matched id.
- `compute_array_append_mutation(doc, array_name, rows) -> Result<MutationPlan>` — thin forward to `compute_add_many_mutation(doc, array, rows, None)`.
- `compute_set_mutation(doc, path, value, ty) -> Result<ScalarMutationPlan>` (in `io.rs`) — clones doc, runs `set_at_path`, returns `{path, old_value, new_value}`.
- `compute_set_json_mutation(doc, path, json) -> Result<ScalarMutationPlan>` — same as above but for the JSON variant.

**New types** (in `items.rs` and `io.rs`):
- `ScalarMutationPlan { path: String, old_value: Option<JsonValue>, new_value: JsonValue }` (in `io.rs`).

**New emitter** (in `output.rs`):
- `pub(crate) fn emit_dry_run_scalar(plan: &ScalarMutationPlan) -> Result<()>` emits `{"ok":true,"dry_run":true,"would_change":{"path":"...","old":<json>,"new":<json>}}`.

**Dispatch wiring** (`cli/dispatch.rs`):
- Mirror the existing pattern from `Cmd::Items(ItemsOp::Apply)` at line 714: read doc, branch on `dry_run`, call compute helper or live mutate_doc.

**Output-shape backwards compat**: existing 3 dry-run subcommands keep their exact byte output. The integration tests at `items_dry_run.rs` still pass.

**Edge-case behaviour to specify and test**:
- `--dry-run` against a non-existent file: dry-run reads via `read_doc`, which respects `--strict-read` for the read-side of the operation. For `items add` against a missing file (no `--strict-read`), the dry-run path bootstraps an empty doc just as the live path would; `--strict-read --dry-run` errors with `kind=not_found` before reaching the compute stage.
- `items add --dedupe-by file,summary --dry-run` when the row matches an existing item: emit `{added:0,skipped:1,...}` populating `MutationPlan.skipped` (the field already exists at items.rs:1066). Without this, a deduped no-op is indistinguishable from `{added:0,...}` for an unrelated reason.
- `--verify-integrity --dry-run`: the read side honours sidecar verification just as in the existing `items remove --dry-run` pattern. Not a new code path.

#### Change 3 — Enumerating-error rewrites (Principle 3)

**Scope (per User Decision)**: comprehensive — rewrite all 47 zero-enumeration sites AND all 23 partial-enumeration sites = ~70 total. Skip only the Tier-3 IO pass-through errors that carry inherited `anyhow` messages.

Breakdown:
- 7 enum rejections (zero-enum) — name the valid op set.
- 15 state-precondition errors (zero-enum) — suggest discovery commands or required fields.
- 8 path-shape rejections (zero-enum) — quote the expected shape.
- 17 other zero-enum sites (missing-field, type-mismatch) — enumerate required fields or expected types.
- 23 partial-enum sites — extend the existing message to also quote what the caller actually passed (e.g. "--json must be a JSON object; got JSON array").

**Rewrite template** (modeled on `dedup.rs:76`):
```
Before: "unknown op `update2`"
After:  "unknown op `update2`; valid ops are: add, update, remove"

Before: "no item with id = R123"
After:  "no item with id = R123; existing ids: ... (run `tomlctl items list <file> --pluck id` to enumerate)"

Before: "--json must be a JSON object"
After:  "--json must be a JSON object (e.g. {\"key\":\"value\"}); got JSON " + crate::convert::json_type_name(&v)

(Use `crate::convert::json_type_name(v: &JsonValue) -> &'static str`, already defined at `convert.rs:416`. Returns lowercase `"null"`/`"bool"`/`"number"`/`"string"`/`"array"`/`"object"`. Do NOT use `serde_json::Value::variant_name()` — it doesn't exist.)
```

**Audit ledger persistence**: the ledger from Agent 3's exploration MUST be persisted at `.claude/flows/glistening-wiggling-valiant/audit-error-rewrites.md` BEFORE Tasks 5 / 6c start. Phase 0 (below, in `## Tasks`) is the explicit pre-flight task that writes the 70-row table (file:line:current_message:rewrite-target). Tasks 5 and 6c cite this file as their input list — without it, the executing agent has no concrete target set and would have to re-perform the audit at runtime, which is the architectural-decision-at-runtime anti-pattern the plan format forbids.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test:  cargo test --manifest-path tomlctl/Cargo.toml
lint:  cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets -- -D warnings
audit: cargo audit --file tomlctl/Cargo.lock
shared-blocks: bash scripts/verify-shared-blocks.sh
smoke: tomlctl capabilities | jq '.commands.items.subcommands.list.flags."--count".type' # → "bool"
```

## Tasks

### Phase 0 — Audit-ledger persistence (single sequential prerequisite)

#### 0. Persist the error-audit ledger [S]
- **Files**: `.claude/flows/glistening-wiggling-valiant/audit-error-rewrites.md` (NEW)
- **Depends on**: —
- **Action**: Write a Markdown table with one row per ~70 audit-flagged error site. Columns: `file:line | current_message | class | proposed_rewrite`. Source the rows from this plan's `## Exploration Notes` → `### Error audit (Agent 3)` section + the inline rewrites listed in Tasks 5 detail. Tasks 5 and 6c read this file to know exactly which sites to touch and what the new message text should be.
- **Detail**: Without this file, Tasks 5/6c are unbounded — the executing agent would have to re-perform the audit at runtime, which violates the "no architectural decisions during execution" rule.
- **Acceptance**: `test -f .claude/flows/glistening-wiggling-valiant/audit-error-rewrites.md` passes. `wc -l` reports ≥ 70 data rows (one per site).

### Phase 1 — Foundations (parallel — independent files)

#### 1. Add `dry_run: bool` fields to clap variants [S]
- **Files**: `tomlctl/src/cli/types.rs`
- **Depends on**: —
- **Action**: Add `#[arg(long, default_value_t = false)] pub(crate) dry_run: bool` to each of: `Cmd::Set`, `Cmd::SetJson`, `Cmd::ArrayAppend`, `ItemsOp::Add`, `ItemsOp::AddMany`, `ItemsOp::Update`. Match the field-position convention from existing `ItemsOp::Remove::dry_run` (types.rs:606-610). **AND** in the same task, update the destructure pattern in each affected dispatch arm at `cli/dispatch.rs:374` (`Cmd::Set`), `:388` (`Cmd::SetJson`), `:410` (`Cmd::ArrayAppend`), `:543` (`ItemsOp::Add`), `:597` (`ItemsOp::AddMany`), `:668` (`ItemsOp::Update`) — bind `dry_run` in the destructure (no branch yet; the `if dry_run` branch lands in Task 6(b)). Without this, `cargo build` after Task 1 fails with E0027 'pattern does not mention field `dry_run`'.
- **Detail**: Do NOT add to `Cmd::Get`/`Cmd::Parse`/`Cmd::Validate`/`ItemsOp::List`/etc. — read subcommands have nothing to dry-run.
- **Acceptance**: `cargo build --manifest-path tomlctl/Cargo.toml` succeeds. `cargo run -- items add foo.toml --dry-run --json '{...}'` parses without "unexpected argument".

#### 2. Implement clap-reflection capability schema [M]
- **Files**: `tomlctl/src/capabilities.rs` (NEW), `tomlctl/src/main.rs` (add `mod capabilities;`)
- **Depends on**: —
- **Action**: Create `pub(crate) fn build_agent_context() -> serde_json::Value` that walks `<Cli as CommandFactory>::command()` recursively (NB: `Command::get_subcommands()` returns only direct children — drive the recursion in `build_agent_context` itself, descending into each `&Command`'s own `get_subcommands()` until empty). For each subcommand, emit `{flags: {...}, mutex_groups: [...]}`. For each flag, emit `{type, required, default?, values?, repeatable}` based on `Arg::get_action()` / `get_value_parser().possible_values()` / `is_required_set()` / `get_default_values()`. Detect repeatable Vec flags via `match arg.get_action() { ArgAction::Append | ArgAction::Count => true, _ => false }` — `Arg::is_multiple()` does NOT exist in clap 4 (removed during the 3→4 ArgAction migration).
- **Detail**: Add a `MUTEX_GROUPS: &[(&str, &[&[&str]])]` const inside `capabilities.rs` for the semantic constraints clap doesn't expose (e.g. `[("items list", &[&["count","count_by","group_by","pluck","count_distinct"]])]`). Walk `Command::get_groups()` first — fall through to the const supplement only for groups that aren't represented there. Also pre-declare `ENUM_VALUES: &[(&str, &[&str])]` covering the project's `ValueEnum` types (`ScalarType`, `DupTier`, `ErrorFormat`, `OpKind` if relevant); consult before declaring a flag's enum values empty (rolls in the Risk #1 fallback as a Task-2 deliverable rather than a discovered-at-runtime fix).
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml` builds. Module-internal unit test `build_agent_context_includes_items_list_count_flag` confirms the JSON shape.

#### 3. Add `compute_*_mutation` helpers for items operations [M]
- **Files**: `tomlctl/src/items.rs`
- **Depends on**: —
- **Action**: Add `compute_add_mutation`, `compute_add_many_mutation`, `compute_update_mutation`, `compute_array_append_mutation` (signatures listed in Approach §Change 2). Each clones `doc`, runs the existing live mutator on the clone, and captures touched ids into a `MutationPlan`. Mirror the pattern of `compute_apply_mutation` at items.rs:1100.
- **Detail**: Add the byte-equivalence unit test for each helper (4 new tests in `#[cfg(test)] mod tests`), modeled on `compute_apply_mutation_new_doc_matches_live_apply_bytes` (items.rs:2829). For `compute_array_append_mutation`, the test should target a non-default array name (e.g. `rollback_events`) to confirm the array name threads through.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml items::tests::compute` reports the 4 new tests passing alongside the 3 existing ones.

#### 4. Add `compute_set_mutation` + `ScalarMutationPlan` + `emit_dry_run_scalar` [M]
- **Files**: `tomlctl/src/io.rs`, `tomlctl/src/output.rs`
- **Depends on**: —
- **Action**: In `io.rs`, define `pub(crate) struct ScalarMutationPlan { path, old_value: Option<JsonValue>, new_value: JsonValue }`. Add `compute_set_mutation(doc, path, value, ty)` and `compute_set_json_mutation(doc, path, json)` — both clone `doc`, then call `crate::convert::navigate(&clone, path)` to capture the pre-mutation value into `old_value` (None when the path doesn't exist yet), then call `crate::convert::set_at_path` (defined at `convert.rs:105`, NOT in `io.rs`) on the clone, and return the populated `ScalarMutationPlan`. Note: `old_value` uses direct `serde_json::Value` encoding — string/int/float/bool/null only; the dry-run path inherits the same scalar-type restriction as live `set` (no datetime mutation today). In `output.rs`, add `pub(crate) fn emit_dry_run_scalar(plan: &ScalarMutationPlan) -> Result<()>` that emits `{"ok":true,"dry_run":true,"would_change":{"path":..., "old":..., "new":...}}` via `print_json_compact`. When `old_value` is `None` (path didn't exist), emit `"old": null`.
- **Acceptance**: New module-internal tests `compute_set_mutation_captures_old_value` and `emit_dry_run_scalar_shape` pass. Existing tests still pass.

#### 5. Apply error-rewrite ledger to non-dispatch files [M]
- **Files**: `tomlctl/src/query.rs`, `tomlctl/src/dedup.rs`, `tomlctl/src/convert.rs`, `tomlctl/src/blocks.rs`
- **Depends on**: —
- **Action**: For each error site flagged in the Phase 1 audit ledger living in these files, rewrite the message per the template in Approach §Change 3 — comprehensive scope (zero-enum + partial-enum sites). Preserve existing tier-C messages verbatim.
- **Detail**: Specifically (anchoring lines from the audit):
  - `query.rs:1682-1685` `"expected KEY=VAL, got `{}`"` → add example `e.g. status=open`.
  - `query.rs:1777/1783` `"--where-has expects a KEY"` → unchanged (already enumerative-ish).
  - `convert.rs:314` `"number `{}` is not representable in TOML"` → "JSON number `{}` is not representable as TOML int or float (must fit i64 or be finite f64)".
  - `convert.rs:547/556` `"type hint `{:?}` doesn't match"` → "type hint `{:?}` rejected; expected one of: int, float (TOML's only numeric types)".
  - `blocks.rs:132` `"blocks verify: no files supplied"` → "blocks verify: no files supplied; pass one or more file paths (e.g. `tomlctl blocks verify a.md b.md`)".
- **Acceptance**: All existing tests pass (rewrites preserve semantic content, just expand prose). New unit tests cover at least one rewrite per category (enum-rejection, state-precondition, path-shape, type-coercion, partial-enum extension) — minimum 5 tests, not 3 — asserting each rewritten message contains the enumerated valid set or the actual-vs-expected pair.

### Phase 2 — Dispatch wiring (depends on Phase 1)

#### 6a. Wire capabilities arm [S]
- **Files**: `tomlctl/src/cli/dispatch.rs:446-460`, `tomlctl/src/cli/types.rs` (FEATURES const)
- **Depends on**: 2
- **Action**: at `cli/dispatch.rs:446-460`, after constructing the existing `{version, features, subcommands}` JSON, add `"commands": capabilities::build_agent_context()` then `print_json(&output)`. Add `"agent_context"` to the `FEATURES` const in `cli/types.rs`.
- **Acceptance**: `cargo build --manifest-path tomlctl/Cargo.toml` succeeds; the capabilities integration test (after the Task 7 update) confirms the `commands` key is present and contains the expected subcommand tree.

#### 6b. Wire dry-run dispatch arms [M]
- **Files**: `tomlctl/src/cli/dispatch.rs` (6 write arms; imports at lines 33-34)
- **Depends on**: 1, 3, 4
- **Action**: For each of `Cmd::Set` / `Cmd::SetJson` / `Cmd::ArrayAppend` / `ItemsOp::Add` / `ItemsOp::AddMany` / `ItemsOp::Update`, add an `if dry_run { ... return; }` branch at the top of the arm. The branch MUST call `read_doc(&file, opts, |doc| ...)` for the read side and operate on a clone — it MUST NEVER call `mutate_doc` on the dry-run path. `mutate_doc` acquires the exclusive lockfile and refreshes the `.sha256` sidecar; both are write-side actions inappropriate for a no-op preview. The reference pattern is `ItemsOp::Apply`'s dry-run branch at `cli/dispatch.rs:714` — it explicitly calls `compute_apply_mutation` against a `read_doc`-loaded `TomlValue`, never `mutate_doc`. Update the imports at `cli/dispatch.rs:33-34` to bring in the new compute helpers (from items.rs and io.rs/output.rs).
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml` passes; `cargo run -- items add fixture.toml --dry-run --json '{"id":"R1"}'` returns the dry-run envelope without writing the file or refreshing the sidecar.

#### 6c. Apply audit rewrites in dispatch/items/io [M]
- **Files**: `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/items.rs`, `tomlctl/src/io.rs`
- **Depends on**: 0 (audit ledger), 6a, 6b
- **Action**: For each entry in `.claude/flows/glistening-wiggling-valiant/audit-error-rewrites.md` whose `file` is one of these three, rewrite the error message per the proposed_rewrite column. Per Agent 3's audit, this covers ~22 sites in items.rs (around lines 198/224/600/606/630/634/642/652/688/737/740/742/780/786/793/797/811/815/830), ~5 sites in dispatch.rs (115/153/201/358/430), and io.rs as flagged. Use `crate::convert::json_type_name` (convert.rs:416) for the partial-enum "got JSON X" rewrites. Skip Tier-3 IO pass-through errors.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml` passes; `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets -- -D warnings` passes.

**Commit boundaries within Phase 2**: commit after 6a, after 6b, after 6c — three commits, not one. Each is independently shippable and rollback-safe (per Agent 4's risk finding on intermediate-commit hygiene).

### Phase 3 — Tests + docs (depends on Phase 2)

#### 7. Extend integration tests [M]
- **Files**: `tomlctl/tests/items_dry_run.rs`, `tomlctl/tests/capabilities.rs`
- **Depends on**: 6a, 6b, 6c
- **Action**: For dry-run: add 6 new integration tests, one per newly-supported subcommand, each asserting (a) JSON output shape, (b) file-bytes-unchanged on disk, (c) sidecar-bytes-unchanged. Mirror the existing test pattern (file/sidecar invariance + stdout shape). Also add edge-case tests: `--dry-run` on a non-existent file (bootstrap behavior), `items add --dedupe-by ... --dry-run` matching an existing row (must populate `MutationPlan.skipped`), `--verify-integrity --dry-run` (sidecar verified before compute). For capabilities: add tests asserting the `commands` key exists, that `commands.items.subcommands.list.flags` enumerates the expected flags with correct types, that `commands.items.subcommands.list.mutex_groups` lists the shape mutex.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --tests` passes including the 6+5 new tests. Total test count rises by ≥11.

#### 8. Update docs + version bump [S]
- **Files**: `claude/skills/tomlctl/SKILL.md`, `tomlctl/README.md`, `tomlctl/Cargo.toml`, `tomlctl/tests/capabilities.rs`
- **Depends on**: 6a, 6b, 6c, 7
- **Action**:
  - SKILL.md: Add a `### Agent-context schema (`tomlctl capabilities .commands`)` section under "Feature-gate with `tomlctl capabilities`". Add `agent_context` row to the existing feature table at SKILL.md:114 (between `strict_read / dry_run` and `backfill_dedup_id / integrity_refresh`). Update the `--dry-run` mentions throughout (currently scoped to remove/apply/backfill) to cover all 9 write subcommands. Add a new recipe to "Common recipes" demonstrating `tomlctl set foo.toml status review --dry-run`.
  - README.md: Update the "Capabilities feature list" section under `## Contracts` — the example JSON at lines 181-191 currently shows `"version": "0.2.0"` (already stale at 0.3.0). Update to `"version": "0.4.0"`, add `"agent_context"` to the `features` array, AND show the new `commands` key with at least one nested example so the README matches the schema users will actually see. Update the "## Design" bullet about dry-run.
  - Cargo.toml: bump `version = "0.3.0"` → `"0.4.0"`.
  - **`tomlctl/tests/capabilities.rs`**: update the strict assertions that pin to today's surface — line 1548-1554 asserts `features.len() == expected.len()` (will fail when `agent_context` is added), and line 1576-1579 asserts the literal string `"0.3.0"` (will fail on the bump). Add `"agent_context"` to the test's `expected` list and update the literal version to `"0.4.0"` in lockstep with Cargo.toml.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` passes (no shared-block files changed by this plan). `cargo build --manifest-path tomlctl/Cargo.toml` produces a `tomlctl --version` output of `tomlctl 0.4.0`.

## Dependency Graph

```
Batch 0 (single):   Task 0                  — audit ledger persistence
Batch 1 (parallel): Tasks 1, 2, 3, 4, 5     — independent foundations (5 files, 1 file each)
Batch 2a (parallel): Tasks 6a, 6b           — capabilities arm + dry-run wiring (disjoint dispatch.rs regions)
Batch 2b (single):  Task 6c                 — audit rewrites (depends on 6a, 6b, 0)
Batch 3 (parallel): Tasks 7, 8              — tests + docs (depend on 6a/6b/6c)
```

Total file count by batch:
- Batch 0: 1 NEW file (audit-error-rewrites.md)
- Batch 1: types.rs, capabilities.rs (NEW) + main.rs, items.rs, io.rs + output.rs, query.rs + dedup.rs + convert.rs + blocks.rs = 5 tasks × ~1.5 files = ~8 unique files
- Batch 2a: dispatch.rs (different sections) + types.rs (FEATURES const) = 2 files across 2 parallel tasks
- Batch 2b: dispatch.rs + items.rs + io.rs = 3 files in one task
- Batch 3: 2 test files + 4 doc/manifest files (incl. tests/capabilities.rs strict-assertion update) = 6 files in two tasks

Max files per batch: 6 (Batch 3). Within the 6-file batch cap.

## Verification

End-to-end:
1. `cargo build --manifest-path tomlctl/Cargo.toml` — must succeed
2. `cargo test --manifest-path tomlctl/Cargo.toml` — all existing tests pass + 11 new tests pass
3. `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets -- -D warnings` — clean
4. `cargo audit --file tomlctl/Cargo.lock` — no new advisories
5. Smoke check: `tomlctl capabilities` JSON output (via the integration test in `tests/capabilities.rs`) contains a `commands` key whose value is an object with subcommand entries. (Avoid `jq` dependency — the integration test in Task 7 already asserts the same shape and has no external prerequisites.)
6. Smoke check: on Windows (the user's primary platform per CLAUDE.md MEMORY), `tomlctl set $env:TEMP\scratch.toml status review --type str --dry-run --allow-outside` (PowerShell). On Unix, `tomlctl set /tmp/scratch.toml status review --type str --dry-run --allow-outside`. The `/tmp` literal does not exist on Windows where this repo is primary.
7. **Reinstall the binary**: `cargo install --path tomlctl --force` after the Cargo.toml version bump — without this, every `tomlctl` invocation in subsequent smoke checks resolves to the previously-installed 0.3.0 binary on PATH, masking acceptance failures (per `CLAUDE.md` 'rerun cargo install when the tomlctl binary version bumps').
8. Bypass-rollback check: `git diff` shows no changes to `claude/commands/*.md` (those are the consumers of tomlctl; this plan should not require any flow-command changes since both `capabilities` and `--dry-run` are additive)
9. `bash scripts/verify-shared-blocks.sh` — shared-block parity unchanged

## Risks

- **Risk**: Clap reflection in Phase 1 Task 2 hits an edge case where `get_value_parser().possible_values()` returns `None` for some `ValueEnum` flags despite the flag being declared with `#[arg(value_enum)]`.
  - **Mitigation**: `capabilities.rs` falls through to a hand-maintained `ENUM_VALUES` const for any flag where reflection returns no `possible_values()`. The const stays small (only `ScalarType`, `DupTier`, `ErrorFormat`, possibly `OpKind`).
- **Risk**: `ScalarMutationPlan`'s `old_value: Option<JsonValue>` shape varies depending on whether the path existed pre-mutation. Tests must cover both branches.
  - **Mitigation**: The Phase 1 Task 4 acceptance criterion includes both branches.
- **Risk**: Audit rewrite messages in Phase 2 Task 6 (sub-piece c) accidentally break a downstream consumer that regex-matches stderr text (e.g. a flow command branching on "no item with id" prose).
  - **Mitigation**: Search `claude/commands/*.md` for any literal substring of the messages being rewritten before changing each one. The audit ledger should annotate each rewrite candidate with "is this string referenced by a consumer?" Document any consumer-facing rewrites in the PR description so they can be reviewed.
- **Risk**: Adding `agent_context` to the `FEATURES` const looks additive but the new `commands` key in the JSON envelope could break a strict-schema consumer.
  - **Mitigation**: README.md's stability contract explicitly says new keys are additive. No consumer in `claude/commands/*.md` does strict-schema validation. Verified via grep.
- **Risk**: `dry_run` flag added to `Cmd::Set` / `Cmd::SetJson` / `Cmd::ArrayAppend` could collide with an existing user habit of passing `--dry-run` and expecting a parse error. (Vanishingly small.)
  - **Mitigation**: None needed — the new behaviour is additive and the flag's default (`false`) preserves all existing call paths byte-for-byte.

