Status: done — the plan store types attempt, gate_round and report arrays, stamped 3 only while any is present

## What changed

- `tomlctl/src/plan/schema.rs`
  - `SCHEMA_VERSION` is now 3, the newest stamp this build reads and the one a store holding any run record is written at; new `BASE_SCHEMA_VERSION = 2` is written when the store holds none, and is what an absent `schema_version` reads as (the same value as before). A const assertion keeps 3 > 2. The refusal stays `schema_version > SCHEMA_VERSION`.
  - `PlanStore` gains `attempts`, `gate_rounds`, `reports` and `write_version()`; `to_toml` stamps `write_version()`, and the three arrays are written after `handover`, each omitted while empty.
  - New records `Attempt` (with `Deviation`, `Tangential`, `CheckRun`, `ProposedHandover`), `GateRound` (with `GateCommand`), `Report` (with `ReportEntry`), and vocabularies `NoteBlock`, `CheckResult`, `GateOutcome`, `CommandOutcome`, `ReportKind`, in the keys and key order of design/records.md, Run records. A gate round requires `milestone`, `round`, `seq`, `at`, `outcome`; a report entry's `needs_person` reads true when absent and `task` reads 0 when absent; `exit` and `attempt.handover` are omitted while unset. Free text is LF-normalised both ways.
  - Helpers `nested`, `required_u32`, `text_table`, and `bool_or` (which `bool_or_false` now calls).
  - Tests: the fixture now holds one of each run record (so it stamps 3); `base_fixture()` is the fixture without them. New tests: each array alone round-trips through rendered text and raises the stamp; all three together round-trip and sit after `handover`; an emptied store returns to stamp 2 and writes exactly as the base fixture; `write_version` per store; optional keys stay absent; absent `needs_person`/`task` defaults; CRLF normalisation. Vocabulary and required-key case lists extended. The existing newer-version test (`SCHEMA_VERSION + 1`, now 4) covers the refusal of a store stamped above what this build knows. 30 `#[test]`s.
- `claude/skills/tomlctl/references/plan-store.md`: the three arrays in the sample, the stamp rule, and a key table each naming the writing verb (`plan run notes`, `plan run gate`, `plan run report`).

## Deviations

- Stamp constants: rather than keep `SCHEMA_VERSION = 2` and add a separate higher constant, `SCHEMA_VERSION` became 3 (the read ceiling) and `BASE_SCHEMA_VERSION = 2` was added. The alternative would have broken `tomlctl/src/plan/store.rs` tests that write `SCHEMA_VERSION + 1` expecting refusal, a file outside this task. `PlanStore::default()` now carries the base stamp, so an empty store still writes and reads as before.
- `attempt.status`, `tangential.kind` and `attempt.handover.reason` are free strings, not vocabularies: they are copied from a worker's note as written, and a typo there should not make the whole store unreadable. Verb-computed values (`block`, `checks.result`, gate outcomes, report `kind`) are closed vocabularies, refused when unknown.
- `duration_s` reads 0 when absent; nested arrays (`deviations`, `tangential`, `checks`, `commands`, `entries`, `files_touched`, `failed_ids`) are always written, like `decision.rejected`.

## Checks

- `rustfmt --edition 2024 --check tomlctl/src/plan/schema.rs`: clean.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean (one constant-assertion warning fixed by moving it to a const assert).
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` (after a `claude/**/*.md` edit): 55 passed.
- Accept, schema types the arrays (grep): pass.
- Accept, reference documents the arrays (grep): pass.
- Accept, schema test count > 23: pass (30).
- Accept, `cargo nextest ... -E 'test(plan::schema)'`: gate's, not run.

## Handover

None.

TANGENTIAL: none
