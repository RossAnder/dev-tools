Status: done — handover reshaped to reason/action/when/done_when, plan store at schema 2, `plan handover set --ndjson` replaces `add`

## What changed

- `tomlctl/src/plan/schema.rs`: new `HandoverReason` vocabulary (restart, permissions, attended, observe, credentials, external) via `vocabulary!`. `Handover` is now `id`, `reason: Option<HandoverReason>`, `action`, `when`, `done_when`, `source_task`, `done`. Read: absent text keys read as `""` (LF-normalised), absent or empty-string `reason` reads as `None`, an unknown `reason` is refused naming the vocabulary; `id` and `source_task` stay required. Write order is id, reason, action, when, done_when, source_task, done, omitting an absent reason and empty text keys. `SCHEMA_VERSION` is 2 and `to_toml` stamps `SCHEMA_VERSION` instead of the version read from disk; module header and the newer-version guard comment updated to match. Tests: fixture and id-reuse literal moved to the new shape; `handover.reason` added to the unknown-vocabulary cases; new tests for absent/empty handover fields (read empty, written without them), handover key order, and a version-1 store written back at the current version.
- `tomlctl/src/plan/records.rs`: `HANDOVER_KEYS` is the full record key list (review keys are `[1..]`, as for decisions and contracts); `HANDOVER_SET_KEYS` drops `done`. `handover_add` replaced by `handover_set`, mirroring `contract`/`upsert_contract`: rows validated under the store lock, a row without `id` mints `H<n>` and needs a non-empty `action` and a `source_task`; a row with `id` updates only the keys it carries, refuses an empty `action`, and is refused when the id is unknown. A `done` key is refused with a message naming `plan handover done <id>`. Prints `{"ok":true,"ids":[...]}`. `record_value` serves the new keys (`reason` as its string, `""` when absent). Unit tests added for mint-then-partial-update and for each refusal leaving the store unchanged; the review-edit refusal case now targets `action`.
- `tomlctl/src/cli/types/plan.rs`: `PlanHandoverOp::Set(PlanHandoverSetArgs)` with `--ndjson` exactly as `PlanContractArgs`; `PlanHandoverAddArgs` removed; doc strings updated.
- `tomlctl/src/cli/mod.rs`: re-export renamed (sorted as rustfmt wants).
- `tomlctl/src/plan/dispatch.rs`: routes `Set` to `records::handover_set`.
- `tomlctl/src/plan/approve.rs`, `tomlctl/src/plan/view.rs`: `h.command` reads replaced by `h.action`; output otherwise untouched for the later reshape.

## Deviations

- approve.rs / view.rs: the JSON key was renamed with the field (`"command"` -> `"action"`), not only the struct read. Alternative was to keep emitting `"command": h.action`; that would print an action under a name the record no longer has. Treated as part of "the field rename" the constraint allows.
- schema.rs: an empty-string `reason` reads as `None` (like `finished = ""` on a run) rather than being refused as an unknown value, so `plan check` reports it with the absent case. Alternative (refuse `""`) would make a hand-blanked reason unreadable instead of reportable.

## Handover

None.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check`: clean (diffs applied by hand with Edit).
- Accept "schema version is 2" (grep): pass.
- Accept "handover carries done_when and no command" (grep): pass.
- Accept "write verb is set, taking ndjson" (grep): pass.
- Accept "schema unit tests pass" (`cargo test --lib plan::schema`): Writes build: yes — left to the gate, not run.

## Known breakage outside this task's files

- `tomlctl/src/plan/target.rs` unit test `every_target_kind_parses_and_prints_back` lists `"handover:H1.command"`; `command` is no longer a handover key, so that parse is now refused and the lib test will fail. One-line fix: change it to `"handover:H1.action"`. No task in this plan holds `target.rs`; the replan should add it (its `PLAN` fixture's `command = ...` line is harmless, the key is dropped on read, but could also become `action`).
- Integration tests under `tomlctl/tests/` and `stage.rs`'s `HANDOVER_REPLACE` remedy text (still `plan handover add --command`) are owned by tasks 2 and 4-6, as the plan says.

TANGENTIAL: bug | tomlctl/src/plan/target.rs | lib test `every_target_kind_parses_and_prints_back` still targets `handover:H1.command`, which no longer parses | no task holds this file, so the milestone gate's lib suite fails until it is changed to `handover:H1.action`
