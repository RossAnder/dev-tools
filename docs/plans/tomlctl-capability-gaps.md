# Plan: Close tomlctl capability gaps — aggregation, output, dedup, safety

**Plan path**: `docs/plans/tomlctl-capability-gaps.md`
**Created**: 2026-04-18
**Status**: Draft (revised after /review-plan)

## Context

A scan of the two most recent conversation transcripts found **529 pipe-chain occurrences** where tomlctl output was post-processed by `jq`/`head`/`sort`/`uniq`/`wc`/`awk`/`sed` — chains the agent-native refactor was supposed to eliminate. Auditing the source showed that 4 of the originally-reported gaps are already implemented (`--limit`, `--offset`, `--distinct`, sort-before-group) — agents just don't know. R40 (already landed) also made `items next-id --prefix` required, closing another gap. The rest are genuine missing primitives or correctness traps:

- **Aggregation gap**: no `--count-distinct <field>` — agents spell it `--pluck f | jq -r '.[]' | sort -u | wc -l` (~140 hits).
- **Output-shape gap**: no `--raw`/`--lines` — agents spell scalar extraction `| jq -r .count` (~35 hits) and pluck-to-lines via manual `jq -r '.[]'`.
- **Ergonomics gap**: `items next-id` with a strict required `--prefix` works, but agents have no way to ask tomlctl "what prefix does this ledger use?" — a `--infer-from-file` flag would close that without weakening R40.
- **Dedup infrastructure unfinished**: `dedup_id` is absent from every ledger; `find-duplicates` can't work cross-ledger.
- **Observability gap**: no `tomlctl capabilities` / structured error JSON / strict-read mode / dry-run.

Closing these lets every flow-command template shrink to pure tomlctl invocations without shell post-processing.

## Scope

- **In scope**: tomlctl binary surface — new flags, new subcommand, new items op modifiers, auto-populated field, README contract section, Cargo.toml version bump to 0.2.0, integration tests.
- **Out of scope**: downstream `claude/commands/*.md` template updates (follow-on plan — see bottom). Lock force-unlock, multi-file integrity batch-check, `--reverse`/`--first`/`--last` aliases (defer — not in transcript pain).
- **Affected areas**: `tomlctl/src/**`, `tomlctl/tests/**`, `tomlctl/README.md`, `tomlctl/Cargo.toml`.
- **Estimated file count**: 10 unique files.

## Research Notes

Exploration mapped the clap derive layout and query pipeline. All line numbers verified 2026-04-18 against HEAD.

- **Clap layout**: `QueryArgs` flattened into `ItemsOp::List` at `tomlctl/src/cli.rs:250-297`. ArgGroup `shape` at `cli.rs:401` enforces `--count | --count-by | --group-by | --pluck` mutual exclusion — new shapes join this group.
- **Query pipeline order**: filter → project → sort → distinct → offset/limit → aggregate/shape (`query.rs:153-283`). Fast-paths at `query.rs:179-216` short-circuit Count/Pluck/CountBy when no sort/distinct/window.
- **`OutputShape` enum** at `query.rs:78-98` is the extension point for new shapes. Match sites that need a new arm when we add `CountDistinct`:
  1. `validate_query` at `query.rs:122-162` (mutex with projection)
  2. Fast-path in `run()` at `query.rs:193-215`
  3. Distinct-dispatch in `run()` at `query.rs:252-260`
  4. Final shape dispatch in `run()` at `query.rs:281-293`
  5. Streaming Array guard in `run_streaming()` at `query.rs:315`
  6. Shape selection in `Query::from_cli_args` at `query.rs:1276-1284`
  7. ndjson-Array guard in `items_dispatch` at `cli.rs:753`
- **`--prefix` is already required** (R40, `cli.rs:519-525`) — comment at `:512-518` explicitly rejects the `R` default.
- **Items write funnels** (all four must carry the `dedup_id` auto-populate hook):
  - `items_add_value_to` at `items.rs:65` (single-item add)
  - `items_update_value_to` at `items.rs:130` (single-item patch — needs recompute logic)
  - `items_apply_to_opts` at `items.rs:194` (batch add/update/remove — loops over ops)
  - `items_add_many` at `items.rs:555` (NDJSON batch add)
- **Dedup state**: `dedup.rs` has tier-A/B/C detection but nothing auto-populates `dedup_id`. Grep of the tree returned zero writes of the field — fully additive. The tier-B fingerprint logic is currently inlined (no standalone function); Task 6a must extract it as `pub(crate) fn tier_b_fingerprint(item: &TomlValue) -> String` before reuse.
- **`--ndjson` orthogonality**: `QueryArgs.ndjson` at `cli.rs:297` is outside the shape ArgGroup (R82). `run_streaming` at `query.rs:306-371` delegates non-Array shapes to `run()`.
- **Exit-code contract**: `main.rs:28-35` uniformly maps any error to exit 1. No per-class codes; this plan preserves that.
- **Reusable functions**:
  - `toml_to_json` in `convert.rs` — used in every shape fast-path.
  - `json_structural_hash` at `query.rs:936` — for dedup fingerprint input.
  - `dedup_preserve_first` at `query.rs:900` — first-occurrence ordering.
  - `ArgMatches::value_source` — explicit-vs-default detection (still current in clap 4.x per docs.rs).
  - `anyhow::Error::chain().find_map(|e| e.downcast_ref::<ErrorKind>())` — traversal through `context` layers (idiomatic per anyhow docs; plain `downcast_ref` only sees innermost).

## Approach

Three architectural decisions:

1. **New aggregation shape `CountDistinct(String)`** rather than relaxing the shape ArgGroup to let `--count` compose with `--pluck --distinct`. Rationale: the ArgGroup enforces correctness cheaply at parse time; a new named shape keeps semantics explicit and mirrors `CountBy`.

2. **`--raw` is a per-subcommand flag**, duplicated on `QueryArgs` and on `Cmd::Get`. Rationale: `--raw` is shape-sensitive; a global would need to propagate through every dispatch arm. Multi-element pluck with `--raw` errors with the exact string `--raw requires single-value output (got {N} items); use --lines for newline-delimited`. Get on table/array errors with `--raw requires a scalar target; got {toml_type}`.

3. **`--lines` is a separate flag, not a clap alias** for `--ndjson`. Rationale: clap aliases hide the alias from `--help`, defeating discoverability — the whole point of fixing this gap.

Additional decisions:

- **`dedup_id` auto-populate** runs in all four write funnels. On `items update`, recompute **only** when a fingerprinted field (`file|symbol|summary|severity|category`) is in the patch AND the patch does not explicitly set `dedup_id`. Otherwise preserve. First-time upgrade of an item that lacks `dedup_id` populates without marking it as a user-intended change — the sidecar refresh is an implicit one-time event, documented in README.
- **Kill switch**: env var `TOMLCTL_NO_DEDUP_ID=1` disables auto-populate globally. Grep-stable and reversible without reverting.
- **Compute/apply split for `--dry-run`**: refactor the existing mutation path into `fn compute_mutation(doc, op) -> MutationPlan` (pure, no I/O) and `fn apply_mutation(plan, path) -> Result<()>` (sidecar + lock + rename). Live path calls both; `--dry-run` stops after `compute_mutation`. This prevents drift between dry-run and real-run logic.
- **`tomlctl capabilities`** emits from a static `FEATURES: &[&str]` const in `cli.rs` (co-located with clap, simplest). Each entry is annotated inline with producing task number (see Task 7). `version` field comes from `env!("CARGO_PKG_VERSION")`.
- **`--error-format json`** is a global flag on `Cli`. Errors written to **stderr** (matches text mode), compact single-line encoding via `serde_json::to_writer`, exit code stays 1. Closed tag-site list: `io.rs` missing-file, `io.rs` sidecar mismatch, `convert.rs` parse errors, `query.rs:validate_query`, `items.rs:items_next_id` prefix validation — all others fall into `kind=other`. Path-leak audit: no home-dir-relative paths in the tree (grep clean).
- **`--dedupe-by` does NOT implicitly include `dedup_id`**. After Task 6 ships, callers who want fingerprint-based dedup can pass `--dedupe-by dedup_id` explicitly, or use `find-duplicates`. Document in Task 5 acceptance.
- **`find-duplicates --across`** supports tier A and tier B only. Tier C is file-scoped by design; passing `--across --tier C` errors with `tier C is file-scoped; use --tier A or --tier B with --across`.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
```

## Tasks

### 1. Add `CountDistinct` shape to query pipeline [M]

- **Files**: `tomlctl/src/cli.rs`, `tomlctl/src/query.rs`
- **Depends on**: —
- **Action**: Add `--count-distinct <FIELD>` to `QueryArgs` (`cli.rs:250-297`). Extend ArgGroup `shape` at `cli.rs:401` by appending `"count_distinct"` to the args list. Add `OutputShape::CountDistinct(String)` variant to `query.rs:78-98`. Update **all seven** match sites (enumerated in Research Notes): `validate_query` (:122-162), fast-path (:193-215), distinct-dispatch (:252-260), shape dispatch (:281-293), streaming guard (:315), `from_cli_args` (:1276-1284), and `cli.rs:753`. Add dedicated fast-path that walks items and hashes plucked-field values into a `HashSet<String>` via `toml_to_json(...).to_string()`, without full per-item materialisation. Treat `build_query` priority as CountDistinct-equal to Pluck — not a Pluck sub-form.
- **Detail**: Output shape: `{"count_distinct": N, "field": "<name>"}`. Null/missing plucked fields excluded from the count (consistent with `--pluck`). `validate_query` mutexes `--count-distinct` with `--select`/`--exclude` (projection makes no sense on an aggregation-only shape).
- **Acceptance**: `tomlctl items list f.toml --count-distinct task_ref` emits the expected object; `--count-distinct x --count` errors at clap parse time; `--count-distinct x --select y` errors via `validate_query`. Unit test in `query.rs::tests` covers null/missing/empty-array/large-cardinality branches. Integration test in `tests/integration.rs` covers the end-to-end CLI surface. Fast-path test asserts no full `toml_to_json` materialisation (structural — assert via counting a recorded invocation, not timing).

### 2. Add `--raw` emitter [S]

- **Files**: `tomlctl/src/cli.rs`
- **Depends on**: 1
- **Action**: Add `--raw` boolean to `QueryArgs` (applies to `Count`, `CountDistinct`, and `Pluck`-with-single-element) and to `Cmd::Get` (scalar targets only). Centralise via `fn emit_raw(v: &JsonValue) -> Result<String>` in `cli.rs`.
- **Detail**: Exact behaviours (error strings are load-bearing — tests assert byte-for-byte):
  - `Count` + `--raw` → emit `{count}\n` (bare integer, trailing newline).
  - `CountDistinct` + `--raw` → emit `{count_distinct}\n`.
  - `Pluck` with N=1 + `--raw` → emit bare value (unquoted if string).
  - `Pluck` with N>1 + `--raw` → error with exact string: `--raw requires single-value output (got {N} items); use --lines for newline-delimited`.
  - `Pluck` with N=0 + `--raw` → error with exact string: `--raw requires single-value output (got 0 items)`.
  - `Get` on scalar + `--raw` → emit unquoted string/number/bool/date, trailing newline.
  - `Get` on table/array + `--raw` → error with exact string: `--raw requires a scalar target; got {toml_type}`.
- **Acceptance**: Integration tests exercise each of the 7 branches above; each error-path test asserts on the exact error string.

### 3. Compose `--ndjson` with `--pluck`; add `--lines` flag [S]

- **Files**: `tomlctl/src/cli.rs`, `tomlctl/src/query.rs`
- **Depends on**: 1
- **Action**: Extend `run_streaming` at `query.rs:306-371` to emit one JSON value per line when `shape==Pluck && ndjson==true`. Add `--lines` as a discrete boolean flag on `QueryArgs` (not a clap alias) that sets `ndjson=true` on Pluck shape; the flag appears in `items list --help`.
- **Detail**: Values stay JSON-encoded (`"foo"` prints with quotes). Composing `--lines --raw` on Pluck strips quotes via the existing `--raw` path. Null/missing fields drop (mirror existing `apply_pluck`).
- **Acceptance**: `--pluck x --lines` writes one JSON value per line; `--pluck x --lines --raw` writes one bare value per line; `--lines` appears in `items list --help`; integration tests cover composition with `--distinct`, `--sort-by`, `--limit`.

### 4. `items next-id --infer-from-file` [S]

- **Files**: `tomlctl/src/cli.rs`, `tomlctl/src/items.rs`
- **Depends on**: —
- **Action**: Add `--infer-from-file` boolean flag to `ItemsOp::NextId` at `cli.rs:519-525`, marked `conflicts_with = "prefix"`. When set, scan existing ID prefixes in the target ledger and:
  - 0 items AND no explicit `--prefix`: error with `--infer-from-file requires a non-empty ledger or explicit --prefix`.
  - 1 distinct prefix: return `{prefix}{max_n+1}`.
  - >1 distinct prefix: error with `--infer-from-file found multiple prefixes ({comma-separated}); pass --prefix explicitly`.
- **Detail**: Extend `items_next_id` in `items.rs:496-514` with a sibling `fn items_infer_and_next_id(doc: &TomlValue) -> Result<String>` that scans prefixes, or extend the existing function to take `Option<&str>` and infer if `None`. R40's `required = true` on `--prefix` is preserved — `conflicts_with` makes them mutually exclusive; neither is required when the other is present (clap handles this via the conflict group).
- **Acceptance**: Integration tests — (a) E-prefix-only ledger with `--infer-from-file` returns `E{n+1}`; (b) mixed-prefix ledger with `--infer-from-file` errors with exact message; (c) empty ledger with `--infer-from-file` and no `--prefix` errors; (d) `--prefix R --infer-from-file` errors at clap (conflict).

### 5. `items add` / `add-many` with `--dedupe-by <fields>` [M]

- **Files**: `tomlctl/src/cli.rs`, `tomlctl/src/items.rs`
- **Depends on**: —
- **Action**: Add `--dedupe-by <F1,F2,...>` to `ItemsOp::Add` and `ItemsOp::AddMany`. Pre-scan the target array for an item matching all field values in the incoming payload; if matched, skip.
- **Detail**: Nested-field support via the `--where`-path walker (NOT the `set`-path walker — they handle array indices differently). The walker lives in `convert.rs`; locate with `grep -n 'fn.*walk\\|fn.*path' tomlctl/src/convert.rs` before starting and cite the exact function name in the implementation PR description. Field comparison uses `JsonValue::eq` on `toml_to_json`'d item value vs incoming patch — same semantics as `--where KEY=VAL` without typed-prefix coercion (document this).
  - Add output on match: `{"ok":true,"added":0,"matched_id":"<id>"}`.
  - Add output on no match: `{"ok":true,"added":1}` (unchanged from today's shape).
  - AddMany output: `{"ok":true,"added":N,"skipped":M,"skipped_rows":[{"row":i,"matched_id":"..."}]}`.
  - **`--dedupe-by` does NOT implicitly include `dedup_id`**. Callers wanting fingerprint-based dedup after Task 6 ships can pass `--dedupe-by dedup_id` explicitly.
- **Acceptance**: Integration tests — (a) running `add --dedupe-by summary,file` twice with identical JSON adds once; (b) different `file` adds twice; (c) AddMany with mixed duplicate/new rows returns correct counts and the row-index list; (d) nested-field path (e.g. `meta.source_run`) dedupes correctly; (e) explicit `--dedupe-by dedup_id` after Task 6 lands works.

### 6. Auto-populate `dedup_id` + `find-duplicates --across <other>` [L]

- **Files**: `tomlctl/src/items.rs`, `tomlctl/src/dedup.rs`, `tomlctl/src/cli.rs`, `tomlctl/README.md`
- **Depends on**: —
- **Action**: Three sub-deliverables in a single task (they share the fingerprint helper):
  - **6a** Extract `pub(crate) fn tier_b_fingerprint(item: &TomlValue) -> String` from the inlined tier-B logic at `dedup.rs:98-147`. Fingerprint = sha256 over normalised `file|symbol|summary|severity|category`, truncated to 16 hex chars (64 bits, ~4B-item birthday bound). Refactor the existing tier-B grouping to call the helper.
  - **6b** Auto-populate `dedup_id` in all four write funnels: `items_add_value_to` (:65), `items_update_value_to` (:130), `items_apply_to_opts` (:194), `items_add_many` (:555). Rules:
    - On add: populate if absent in payload.
    - On update: recompute only when a fingerprinted field is in the patch AND the patch does not explicitly set `dedup_id`.
    - On update of a pre-existing item that lacks `dedup_id` but the patch doesn't touch fingerprinted fields: leave absent (do NOT populate silently — that would be the one-time-upgrade case, which we handle via explicit `items backfill-dedup-id` in Task 11).
    - Gated by env var: `TOMLCTL_NO_DEDUP_ID=1` short-circuits every auto-populate path (rollback lever).
  - **6c** Add `--across <path>` to `ItemsOp::FindDuplicates`. Load the second ledger, union items with `source_file` tagged on each (e.g. `{"source_file":"a.toml","id":"R5",...}`), run the selected tier over the union. `--across --tier C` errors with `tier C is file-scoped; use --tier A or --tier B with --across`.
- **Detail**: `dedup_id` is a string field; serialises as a normal TOML quoted key-value. PROGRESS-LOG rendering is safe — `plan-update.md:211-223` hard-codes column lists, so `dedup_id` never leaks into rendered output. README gets a "Dedup fingerprint" subsection covering: the fingerprint algorithm, the 16-hex truncation bound, the recompute-on-update rule, the kill switch env var, the non-fingerprint-field-patch invariance, and two example fingerprint diffs.
- **Acceptance**: Integration tests — (a) freshly-added item has `dedup_id` matching `tier_b_fingerprint`; (b) `items update --json '{"summary":"new"}'` recomputes; (c) `items update --json '{"status":"fixed"}'` preserves (non-fingerprint patch); (d) `items update --json '{"dedup_id":"explicit"}'` preserves the explicit value regardless; (e) `items update --json '{"summary":"new","dedup_id":"explicit"}'` keeps explicit; (f) env var `TOMLCTL_NO_DEDUP_ID=1` suppresses all auto-populate; (g) `find-duplicates --across other.toml --tier B` returns cross-ledger matches with `source_file` tags; (h) `find-duplicates --across x.toml --tier C` errors with exact message.

### 7. `tomlctl capabilities` subcommand + version bump [S]

- **Files**: `tomlctl/src/cli.rs`, `tomlctl/Cargo.toml`
- **Depends on**: 1, 2, 3, 4, 5, 6
- **Action**: Bump `tomlctl/Cargo.toml` version `0.1.0` → `0.2.0` (semver-relevant new surface). Add `Cmd::Capabilities` variant emitting JSON:
  ```json
  {"version":"0.2.0","features":[...],"subcommands":[...]}
  ```
  `features` from a static const in `cli.rs`, each entry annotated inline with producing task:
  ```rust
  const FEATURES: &[&str] = &[
      "count_distinct",         // T1
      "raw",                    // T2
      "lines",                  // T3
      "infer_prefix",           // T4
      "dedupe_by",              // T5
      "dedup_id_auto",          // T6b
      "find_duplicates_across", // T6c
      "capabilities",           // T7
      "error_format_json",      // T8
      "strict_read",            // T9
      "dry_run",                // T10
      "backfill_dedup_id",      // T11
  ];
  ```
  `subcommands` enumerated as a static list.
- **Detail**: `version` field via `env!("CARGO_PKG_VERSION")` so Cargo.toml is the single source of truth. Help-text strings on new flags match feature names verbatim where possible (e.g. `--count-distinct` help begins `"Aggregate: count distinct values of FIELD"`).
- **Acceptance**: `tomlctl capabilities` output parses as JSON; integration test asserts every feature name present; integration test asserts `version` equals `0.2.0`; `--help` snapshot test for each new flag asserts its help-text substring survives.

### 8. `--error-format json` + `ErrorKind` [M-leaning-L]

- **Files**: `tomlctl/src/main.rs`, `tomlctl/src/cli.rs`, `tomlctl/src/io.rs`
- **Depends on**: —
- **Action**: Add global `--error-format {text,json}` flag on `Cli` root (default `text`). Introduce `ErrorKind` enum (`io | parse | integrity | validation | not_found | other`). Tag a closed list of call sites:
  - `io.rs` — missing-file path (`kind=not_found`)
  - `io.rs` — sidecar hash mismatch (`kind=integrity`)
  - `convert.rs` — TOML parse error (`kind=parse`)
  - `query.rs:validate_query` — query mutex violations (`kind=validation`)
  - `items.rs:items_next_id` — prefix validation (`kind=validation`)
  Every other bail site falls into `kind=other`.
- **Detail**: `main.rs:28-35` catches the `anyhow::Error`, walks `Error::chain().find_map(|e| e.downcast_ref::<ErrorKind>())` to find the deepest tagged kind (plain `downcast_ref` only sees the innermost error — insufficient when `context` wraps). When json-mode, emit to **stderr**, compact single-line via `serde_json::to_writer`:
  ```json
  {"error":{"kind":"not_found","message":"...","file":"..."}}
  ```
  Exit code stays 1 regardless of format.
- **Acceptance**: Integration tests per tagged kind (inject missing file, parse error, sidecar mismatch, query mutex violation, prefix validation failure); text-mode regression test asserts unchanged prose; `--error-format json` with an untagged error emits `kind=other`. Grep assertion: `grep -c 'bail!' tomlctl/src/**/*.rs` count is baseline-captured so an explosion on the next PR is flagged.

### 9. `--strict-read` + missing/zero/corrupt contract in README [S]

- **Files**: `tomlctl/src/cli.rs`, `tomlctl/src/io.rs`, `tomlctl/README.md`
- **Depends on**: 8 (so the error has `kind=not_found`)
- **Action**: Add `--strict-read` to `ReadIntegrityArgs` at `cli.rs:192-200`. When set and file missing, raise error tagged `kind=not_found`. Default behaviour (empty-array) unchanged for back-compat. README gets a "File state contract" subsection documenting three states (missing, zero-byte, parse-error) × two modes (default, `--strict-read`).
- **Acceptance**: Missing-file integration test — without flag returns `[]`; with flag errors; with flag + `--error-format json` emits the structured envelope with `kind=not_found`; layered with `--verify-integrity` test confirms `--strict-read` fires before sidecar check (missing file → not_found, not integrity).

### 10. `--dry-run` on `items remove` and `items apply` via compute/apply split [M]

- **Files**: `tomlctl/src/cli.rs`, `tomlctl/src/items.rs`, `tomlctl/src/io.rs`
- **Depends on**: —
- **Action**: Refactor the mutation path into two pure-ish halves before adding the flag:
  1. `pub(crate) fn compute_mutation(doc: &TomlValue, ops: &[Op]) -> Result<MutationPlan>` — pure, no I/O, returns a `MutationPlan { added: Vec<Id>, updated: Vec<Id>, removed: Vec<Id>, new_doc: TomlValue }`. Runs the `--no-remove` gate, the dedupe-by gate, the dedup_id auto-populate logic — all of it. Errors identically to a real run.
  2. `pub(crate) fn apply_mutation(plan: MutationPlan, path: &Path) -> Result<()>` — does `.lock` acquisition, atomic tempfile + rename, sidecar write. Pure I/O.

  The live path calls both in sequence. `--dry-run` on `ItemsOp::Remove` and `ItemsOp::Apply` stops after `compute_mutation` and emits:
  ```json
  {"ok":true,"dry_run":true,"would_change":{"added":N,"updated":N,"removed":N,"ids":[...]}}
  ```
- **Detail**: Same-input diff equals actual-mutation diff on the same fixtures — this invariance is the whole point of the factoring. `--dry-run --no-remove --ops [... remove op ...]` errors identically to the real run (the `--no-remove` gate lives in `compute_mutation`).
- **Acceptance**: Integration tests — (a) `remove --dry-run <id>` leaves file byte-identical AND `.sha256` mtime unchanged; stdout shows `would_change.removed=["<id>"]`; (b) subsequent `remove <id>` (no dry-run) actually removes; (c) `apply --dry-run --ops [...]` summarises mixed add/update/remove counts; (d) `--dry-run --no-remove` with a remove op errors with the same `--no-remove` error message as a real run; (e) invariance test: run `compute_mutation` on a fixture, serialise `plan.new_doc`, compare against a file produced by a real `apply_mutation` on the same fixture — bytes identical.

### 11. `items backfill-dedup-id <file>` subcommand [S]

- **Files**: `tomlctl/src/cli.rs`, `tomlctl/src/items.rs`
- **Depends on**: 6
- **Action**: Add `ItemsOp::BackfillDedupId { file, integrity }` that walks all items in the ledger, computes `tier_b_fingerprint` for any item lacking `dedup_id`, writes the updated ledger atomically via the compute/apply split from Task 10. Idempotent — re-running is a no-op for items that already have `dedup_id`.
- **Detail**: This is the explicit, auditable upgrade path for pre-Task-6 ledgers. The README's "Dedup fingerprint" subsection (added in Task 6) references this subcommand. Honours `TOMLCTL_NO_DEDUP_ID=1` (no-op when set; exits 0 with `{"ok":true,"backfilled":0,"reason":"disabled-by-env"}`).
- **Acceptance**: Integration tests — (a) ledger with N items, none with `dedup_id` → backfill adds `dedup_id` to all N; (b) ledger where some items already have `dedup_id` → backfill only touches the missing ones; (c) env var set → no-op with explanatory output; (d) backfill + subsequent `items list --pluck dedup_id --distinct --count-distinct dedup_id --raw` returns N.

## Execution Order

All tasks touch `cli.rs`, which is a single 1177-line file — parallel agents on it will produce merge conflicts. The dependency graph is flattened to **strict sequential order**:

```
4 → 5 → 6 → 10 → 11 → 8 → 9 → 1 → 3 → 2 → 7
```

Rationale: Task 4 is smallest and warm-up; 5 and 6 extend items.rs independently of query changes; 10 needs the compute/apply split landed before 11 can reuse it; 8 (error tagging) before 9 (which depends on `kind=not_found`); 1 before 3 and 2 (which both depend on the `CountDistinct` shape landing); 7 last so its feature list is complete.

Tasks 4, 5, 6, 8, 10 are logically independent and CAN be assigned to separate agents only if writes to cli.rs are serialised through a queue (e.g. one agent pushes a PR, the next rebases). In practice, a single agent running them in sequence is simpler.

## Verification

End-to-end, one smoke test per task (mirrors the task list 1:1 so silent regressions are caught):

1. `cargo build --manifest-path tomlctl/Cargo.toml --release` — clean build.
2. `cargo test --manifest-path tomlctl/Cargo.toml` — all existing + ~28 new tests pass.
3. `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` — clean.
4. `cargo install --path tomlctl` — install 0.2.0 binary.

Per-task smoke (run against a real `.claude/flows/*/execution-record.toml`):

- **T1**: `tomlctl items list <record> --where type=task-completion --where status=done --count-distinct task_ref --raw` → bare integer (the exact value previously computed by the 4-stage pipe chain).
- **T2**: `tomlctl get <context.toml> tasks.total --raw` → bare integer, no quotes.
- **T3**: `tomlctl items list <record> --pluck task_ref --lines` → one JSON value per line.
- **T4**: `tomlctl items next-id <record> --infer-from-file` → returns `E{n+1}` against an E-ledger.
- **T5**: `tomlctl items add <record> --json '{...}' --dedupe-by summary,file` run twice → first succeeds, second returns `added:0,matched_id:...`.
- **T6a**: `tomlctl items add <record> --json '{"file":"x.rs","summary":"y"}' && tomlctl items list <record> --pluck dedup_id --raw --limit 1` → a 16-hex string.
- **T6b**: `TOMLCTL_NO_DEDUP_ID=1 tomlctl items add <record> --json '{...}' && tomlctl items list <record> --pluck dedup_id --limit 1` → `[null]` or empty.
- **T6c**: `tomlctl items find-duplicates <review.toml> --across <optimise.toml> --tier B` → cross-ledger matches with `source_file`.
- **T7**: `tomlctl capabilities | jq -e '.features | index("count_distinct")'` → non-null (jq allowed here because capabilities output is intentionally JSON-structured for programmatic consumption).
- **T8**: `tomlctl items list /nonexistent/file.toml --error-format json` → stderr contains `{"error":{"kind":"not_found",...}}`.
- **T9**: `tomlctl items list /nonexistent/file.toml --strict-read` → exit 1; `tomlctl items list /nonexistent/file.toml` → exit 0 with `[]`.
- **T10**: `tomlctl items remove <record> R1 --dry-run && sha256sum <record>` → hash unchanged.
- **T11**: `tomlctl items backfill-dedup-id <legacy-ledger>` → fills all missing dedup_ids idempotently.

## Risks

- **Fast-path regression in `CountDistinct`** (T1) — if the new shape falls through to the generic pipeline instead of its dedicated fast-path, perf regresses on large ledgers. Mitigation: acceptance includes a structural fast-path assertion (count recorded `toml_to_json` invocations on a synthetic fixture).

- **`build_query` priority ladder silently collapses `--pluck x --count-distinct y`** (T1) — the `shape` ArgGroup should reject this at parse time, but the priority-ladder code at `query.rs:1276-1284` historically treated Count > CountBy > GroupBy > Pluck. Verify CountDistinct is equal-precedence with Pluck. Mitigation: parse-time mutex test.

- **`dedup_id` recompute on `items update`** (T6) — if a user patches an unrelated field (e.g. `status`), don't recompute. Only recompute if a fingerprinted field is in the patch. Mitigation: five explicit branch tests in T6 acceptance. Kill switch env var (`TOMLCTL_NO_DEDUP_ID=1`) allows disable-without-revert if the fingerprint logic has a bug.

- **One-time sidecar churn from T6 rollout** — the first `items update` to a legacy item that lacks `dedup_id` would rewrite `.sha256` even for an unrelated patch. Mitigated by NOT auto-populating on update when the patch doesn't touch fingerprinted fields (see T6 detail). Explicit upgrade via T11 (`items backfill-dedup-id`) is the canonical path.

- **`--dry-run` divergence** (T10) — addressed structurally by the `compute_mutation` + `apply_mutation` split. Acceptance includes a byte-identity invariance test between compute-only and compute+apply paths.

- **`--error-format json` scope creep** (T8) — mitigated by the closed tag-site list (5 sites). Anything else falls into `kind=other`. Grep counter baseline prevents an explosion on the next PR.

- **64-bit fingerprint collision bound** (T6) — sha256 truncated to 16 hex chars gives a ~4B-item birthday bound. For review/optimise ledger scale (hundreds of findings per file, tens of files) this is ample. README documents the bound and recommends explicit `dedup_id` for adversarial cases. Not switching to BLAKE3 mid-plan (the hot path is not hash-bound; `sha2` is already a dependency).

- **README drift** — four tasks (6, 7, 9, + the capabilities versioning note) touch README. Keep touches narrow: single "Contracts" section with three subsections (Dedup fingerprint, File state contract, Capabilities feature list).

## Follow-on plan (not in this scope)

After 0.2.0 lands and `cargo install --path tomlctl` is re-run, a separate plan updates downstream command templates:

- `claude/commands/plan-update.md`: replace `--pluck X | jq -r '.[]' | sort -u | wc -l` with `--count-distinct X --raw`; drop the "verify pluck output shape" hedge.
- `claude/commands/implement.md`: replace `| head -N` with `--limit N`; adopt `--dry-run` in rollback-preview.
- `claude/commands/review-apply.md`, `claude/commands/optimise-apply.md`: `--dry-run` previews; `find-duplicates --across` for cross-ledger dedup.
- `claude/commands/review.md`, `claude/commands/optimise.md`: `--count-distinct` replaces `| wc -l` chains.
- `claude/commands/plan-new.md`: reference `tomlctl capabilities` for downstream feature-gating; adopt `items backfill-dedup-id` as a one-time migration step where applicable.
- `CLAUDE.md`: no change expected — `cargo install --path tomlctl` step already documented.

Estimated ~7 files, all markdown edits, ~S effort each.
