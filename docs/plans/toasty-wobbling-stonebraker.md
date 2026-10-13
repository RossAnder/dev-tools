# Plan: Harden the review-v2 store before v2 plan-new/implement

**Plan path**: `docs/plans/toasty-wobbling-stonebraker.md`
**Created**: 2026-10-13

## Summary

Gives review-v2 finding status a single typed owner (`FindingStatus` with per-status companions and predicates, one status-write helper) and migrates every review module onto it. It fixes the reopen-then-sync defect: a reopen retires the v1 link into a new `retired_links` field, sync and export skip retired links, a re-export mints a regression, and a v1 reopen now syncs back. It also makes `review plan --ndjson` replay-safe, adds `--set-json KEY=@path` to every field-flag verb, bounds the survey's git window, and fits the finder's coverage note into the 600-character cap. Per your decisions: it adds a `review-finder-cf` agent with `omitClaudeMd` that the Workflow runs as an ordinary cell, lets `triage --convert` mint a finished run's advisory into a named open run, and keeps glimpse on the export-v1 bridge, documented. Four extra backlog items are folded in. Scrutinise first: Approach, "Status owner and companions" (the same-status write rule) and "Retired links" (the store-schema change, the export rule for a retired settled row, and the once-only v1 reopen), Approach, "Context-free finder cell" (how base and baselines reach the cell), and Risks (about 50 files, two checkpoints).

## Context

The review-system-v2 review left 11 related backlog items (2026-10-13). Their common root is that the v2 store has no owner for a finding's lifecycle. Statuses are bare `String`s; four modules (add, sync, triage, verdict) declare seven status-subset and companion constants, and digest and matching carry inline sets, which already disagree. One visible consequence: a `triage --reopen` of an exported finding is silently undone by the next `sync-v1`, which runs at every run start. The remaining items are write-path fragility (non-idempotent `plan --ndjson`, `$(cat)` payload inlining, an unbounded git window that settles a signal on garbage), an over-long finder note, and three open design questions. Settle them before building v2 `plan-new`/`implement` on the same store pattern, so the new stores copy a sound lifecycle and write discipline.

## Scope
- **In scope**: B-faca904c, B-8a4b923f, B-1fc81407, B-e8b56feb, B-bd78e505, B-d8d355f4, B-de835331, B-c2c975d1, B-be44a578, B-cd31ec02, B-4a72f1a6, plus folded B-8caf86b0, B-6601255f, B-9e96ce7c, B-53bf0848.
- **Out of scope**: glimpse code (B-cd31ec02 is settled by documentation), retyping `Cell.status` / `Cell.agent` / verdict fields, B-615a500d, B-e4c1a22a, B-e922f5cb, B-ef306556, B-771b57e7.
- **Affected areas**: `tomlctl/src/review/`, `tomlctl/src/fields.rs`, `tomlctl/src/io.rs`, `tomlctl/src/cli/types/review.rs`, `tomlctl/tests/`, `tomlctl/README.md`, `claude/agents/`, `claude/workflows/review-v2.js`, `claude/commands/review-v2.md`, `claude/skills/tomlctl/`, `claude/skills/review-v2-engine/`, `claude/skills/review-v2-method/`, `claude/skills/review-v2-lenses/`, `claude/skills/flow-contract-task-store/SKILL.md`, `scripts/shared-blocks.toml`, `CLAUDE.md`.
- **Not edited**: the bound lens, method and check entries under `claude/skills/review-v2-lenses/` that ask for one coverage-note line per item. Editing them rolls their content hashes, which `review stats` groups on; task 18 states the precedence rule in the method skill instead.

## User Decisions

1. **Context-free correctness cell**: new agent + Workflow cell. Add `review-finder-cf` (`omitClaudeMd: true`, body kept identical to review-finder); the cell names it as its agent, the store accepts it, `review show --for finder` omits the pack for it, and review-v2.js runs it like any other cell. _Prompted by_: omitClaudeMd is definition-only (Research Notes, "Claude Code subagent context"); review-v2.md stage 4 hand-dispatches the cell.
2. **Glimpse and v2 findings**: defer and document. Keep export-v1 as the bridge, document it, and resolve B-cd31ec02 as decided. _Prompted by_: LedgerKind / scopes assume v1 `items` rows (Exploration Notes, "Decision items").
3. **Convert on a finished run**: convert into a named open run. A finished run's advisory converts when the caller names an open run, and the replacement is minted there as `unverified`. Without one, the refusal stays. _Prompted by_: triage.rs `replacement_row` refusal and `finish_row` stamping.
4. **Finder note vs 600 cap**: compress the ask. A fixed terse note shape fits 600, and the cap is unchanged. _Prompted by_: review-finder.md:47 asks for roughly 900–1,200 characters.
5. **Backlog fold-in**: B-8caf86b0, B-6601255f, B-9e96ce7c, B-53bf0848, together with the 11 requested items.
6. **Checkpoint cadence**: `milestones`.
7. **`--set-json @path` scope**: every verb, in the shared `crate::fields::build`.

### Phase 5 outcome
Skipped: the answers introduced no library or API topic beyond Research Notes (omitClaudeMd, git windows, jiff). B-6601255f's open question ("list or refuse") was decided in Phase 6: v1 `v2_ref` becomes string-or-array. See Approach, "Retired links".

## Approach

### Status owner and companions
`tomlctl/src/review/fields.rs` (the review catalogue, not the crate-level `tomlctl/src/fields.rs`) gains `str_enum!(FindingStatus { … })` over the 12 statuses (catalogue.rs already owns a type called `Status`). `FINDING_STATUSES` stays as `FindingStatus::VALUES` for row validation. The model separates three kinds of field:
- **Status companions**, owned by exactly one status, from `FindingStatus::companions()`:
  - `merged` → `merged_into`
  - `parked` → `park_reason`
  - `dismissed` → `dismiss_reason`
  - `deferred` → `defer_reason`, `defer_trigger`
  - `backlogged` → `backlog_ref`
  - `converted` → `converted_to`
  - `resolved` → `resolution`, `resolved`
  - `open` → `reopen_reason`
  - every other status → none

  `ALL_COMPANIONS` is derived from this table.
- **Link fields** `exported_as` and `export_ledger`. They are not companions: a sync deliberately keeps them across exported → resolved/dismissed/deferred, and review_sync `fixed_v1_row_resolves_v2_finding` asserts that.
- **`retired_links`**, a new catalogue field (see below).

Predicates replace every per-module list:
- `is_settled` — superseded, merged, converted, exported
- `is_reopenable` — dismissed, deferred, parked, backlogged, refuted, resolved
- `is_suppressing` — dismissed, deferred, parked, backlogged, converted
- `is_syncable` — open, exported, resolved, dismissed, deferred
- `decided_by_verdict` — unverified, open, refuted

Each predicate's set equals today's constant, so behaviour does not move. Inline sets that match no predicate stay local, as `matches!` over `FindingStatus` variants: digest's `{superseded, merged, unverified}` in `audit_sample` and `{superseded, merged, refuted}` in `live`, and matching's `{superseded, merged}` in `candidates`. Triage's reopen refusal keeps an ordered slice of the reopenable statuses, so its message text and order are unchanged. Digest's `DISPOSITIONS` is a display list, not a status subset, and stays a literal.

`tomlctl/src/review/store.rs` adds these methods on `Finding`:
- `status()` returns `FindingStatus`.
- `set_status(status, companions)` is the only status write. When the status changes or companions are given, it removes every `ALL_COMPANIONS` key, then writes the given ones. A same-status write with no companions (a verdict that leaves a reopened finding `open`) keeps the existing companions. It never touches link fields or `retired_links`. A caller carrying a companion forward (sync's kept `resolved` date) reads it before the call.
- `retire_link()`, `is_retired(ledger, v1_id)` and `unretire_link(ledger, v1_id)` own `retired_links` (see "Retired links").

Migration goes accessor-first. Each module migrates in its own file-disjoint task, and only then does `Finding.status` flip from `String` to `FindingStatus` (task 10). That keeps every intermediate commit compiling. The shared `invalid(msg)` helper (B-8caf86b0) moves to `tomlctl/src/review/mod.rs` as `pub(super)`. Each migration task deletes its file's copy; task 7 also replaces the `invalid` closure in `tomlctl/src/review/schema_out.rs`. The helpers tasks 1 and 2 add have no non-test caller until tasks 3–9 and 14 land, so no checkpoint falls between them (clippy's `dead_code` under `-D warnings`).

### Retired links
- **Reopen.** A `triage --reopen` of a finding carrying `exported_as` moves `<export_ledger>#<exported_as>` onto `retired_links`, a computed string-list field with a catalogue entry in `tomlctl/src/review/fields.rs`, and removes both link fields. This happens through a `Finding::retire_link()` helper.
- **Sync.** `sync::apply` skips any link whose `<ledger>#<v1 id>` is retired for that finding, with reason `link retired by reopen`. Without the list, a reopened finding looks exactly like the "export whose v2 write never landed" case that `SYNCABLE` admits `open` for. `stats::v1_links`, the other `linked_rows` consumer, skips retired links the same way, so `review stats --outcomes` does not count a retired row beside its regression.
- **Export.** `export::decide` excludes retired rows from the `v2_ref` match. Retired rows still take part in the merge rule, with one change: a retired row whose v1 status is settled (wontfix, wontapply, verified-clean, deferred) does not yield `Settled`. So for a fixed or settled retired row, the reopened finding mints a new v1 row with `related = [old]` (`Regression`). A retired row that is open again in v1 yields `Reuse`, and `mark_exported` then calls `Finding::unretire_link`, so sync reads that link again.
- **Second link (B-6601255f).** v1 `v2_ref` is read as a string or an array. When export reuses an open v1 row whose `v2_ref` names a different finding, it writes an array holding both rather than overwriting. `sync::linked_rows` and `decide` test membership, and `linked_rows` still yields one `Linked` per named finding, so its item shape is unchanged for `stats`.
- **v1 reopen (B-1fc81407).** When a v1 row is `open` with a non-empty `reopen_rationale`, its finding is at a sync status, and the finding's `by = "sync"` verdict does not already record that rationale, sync moves the finding to `open` with `reopen_reason = "v1: <rationale>"`, using `SYNC_REASON_PREFIX`, and records the rationale in that verdict. The rationale persists on the v1 row while it stays open, so without the check every later `sync-v1` would undo a v2 disposition. `set_status` clears stale companions on every sync write that changes status.

### Write-path fixes
- **`plan --ndjson` replay (B-e8b56feb).**
  - Inside `cells.rs::append`'s lock, the prepared rows are compared with the run's newest wave. The key is `(run, slice in order, concerns, pass, question, agent)`.
  - If that wave's cells are all still `assigned` and match the incoming set exactly, the call returns those cells with `unchanged: true` and mints nothing. Anything else mints a new wave as today.
  - Exploration found the `os error 5` path cannot leave the store written (io.rs commits the sidecar before the TOML). The duplicate therefore comes from replaying a call that succeeded. The "retry after the sidecar rename failed" comments in cells.rs, verdict.rs and triage.rs are corrected to say so.
- **`--set-json KEY=@path` (B-bd78e505).** `tomlctl/src/fields.rs::build` reads a value starting with `@` as a JSON file. It reuses io.rs's `at_file_path`, exposed as `pub(crate)`. JSON never starts with `@`, so the form is unambiguous.
- **Survey window (B-d8d355f4).**
  - `Detect::parse` refuses a window above 36,500 days.
  - `count_in_window` and `commits_per_file` compute the cutoff as `crate::time::now().checked_sub(SignedDuration::try_from_hours(days * 24))` and pass `--max-age=<secs>`. A cutoff at or before 0 passes no filter.
  - This follows Research Notes, "git log time windows". `Span` days do not work on `Timestamp`.
- **Run `incomplete` (B-9e96ce7c).** It becomes a Run catalogue field, so run finish writes it through the catalogue and not into `rest`.

### Context-free finder cell
- **Store.**
  - `tomlctl/src/review/fields.rs` adds `FINDER_CF = "review-finder-cf"` to `CELL_AGENTS`, plus `is_finder(agent)`, so finder-only logic accepts both agents.
  - No field marks a cell as context-free (`claude/commands/review-v2.md`, stage 3 "Store record"), and the lens frontmatter has no variant key. So `prepare_cell` in `tomlctl/src/review/cells.rs` accepts `review-finder-cf` only on a cell whose `expected_agent` is `FINDER` and whose concerns are exactly `["correctness"]`; every other cell refuses it.
  - `query.rs::assignment()` omits `pack` for that agent, keeps `baselines` (so the cell raises nothing the linter already reports), and adds the run's `base`, which the stage-4 prompt used to carry by hand.
- **Agent.** `claude/agents/review-finder-cf.md` sets `omitClaudeMd: true`. Its body is held byte-identical to review-finder.md through a shared-block parity entry in `scripts/shared-blocks.toml`, and it is registered in the `research-delivering` and `review-v2-store-writer` blocks. Because the body is shared, review-finder.md gains a branch: when the assignment carries no pack, the cell is context-free, so it reads the slice as `git diff <base>` using the assignment's `base` and skips the prior-findings read.
- **Workflow.** `review-v2.js` adds the agent to `CELL_AGENTS` and drops the `pack:` line for it. The Agent-wave fallback preamble in `claude/skills/review-v2-engine/references/agent-waves.md` drops it the same way, in both the wave cell prompt and the mop-up re-dispatch.
- **Carrier.** `review-v2.md` composes the cell with that agent and deletes stage 4's hand-dispatch and its `check-<wave>-cf` batch.
- **Constraint.** The cf agent keeps preloaded skills, but no CLAUDE.md, rules or memory reach it. Anything it needs must already be in its body or skills.

### Convert into an open run
`review triage --convert` gains `--run <open run id>`, in `tomlctl/src/cli/types/review.rs`. When the advisory's own run is finished, the replacement is minted with `run` and `seen_in` set to the named open run, `cell` kept from the advisory as provenance, and status `unverified`. The orchestrator verdict is filed under the named run, routing uses the named run's catalogue and depth, and a `--run` whose family differs from the advisory's is refused. It is then held to that run's verification and finish gate. With no `--run`, the existing refusal stands. When the advisory's run is open, `--run` must be absent or name that same run.

### Finder note
`review-finder.md` asks for a fixed note shape:
- counts: files read in full and checks run
- ruled-out candidates, as `file:symbol — reason`
- the reason for a `thin` or `uncovered` seal

It drops "a line per passing check". `review-v2-method/SKILL.md` drops "a line per clean probe answer" in favour of a count, and states the precedence rule: the note is at most 600 characters, and where a bound lens, method or check asks for one note line per sweep, probe, task or pair, a count plus the failing items satisfies it. `review-sweeper.md`'s own note ask moves to the same counts. All four v2 agent bodies switch from `--note -` / `--rationale -` stdin to the `--note-file` / `--rationale-file` twins (B-53bf0848).

## Success Criteria
- forward: no per-module status subset constant remains in `tomlctl/src/review/`. Check: `grep -nE 'const (SETTLED|REOPENABLE|COMPANIONS|SYNCABLE|SYNC_COMPANIONS|SUPPRESSING|DECIDED_BY_VERDICT)\b' tomlctl/src/review/*.rs` prints nothing (today: 7 hits across add, sync, triage, verdict).
- forward: the finding status field is typed. Check: `grep -qE '^[[:space:]]*pub\(crate\) status: FindingStatus,' tomlctl/src/review/store.rs` exits 0 (today: exit 1, and still exit 1 after task 2's accessor).
- forward: no review module defines its own `invalid` helper. Check: `grep -l '^fn invalid(' tomlctl/src/review/*.rs` prints nothing (today: 9 files), and `grep -n 'let invalid' tomlctl/src/review/schema_out.rs` prints nothing (today: 1 hit).
- forward: every behavioural goal has its test: reopen-then-sync and reopen-then-export (B-8a4b923f), the v1 reopen syncing back once (B-1fc81407), the second link (B-6601255f), replay-safe `plan --ndjson` (B-e8b56feb), `--set-json @path` (B-bd78e505), the window cap (B-d8d355f4), convert into an open run (B-4a72f1a6), and the pack-less cf assignment (B-c2c975d1). Check: the `success:` grep below counts all 12 test functions in `tomlctl/tests/` (today: 0); the `test:` suite run is what shows they pass.
- forward: v2 agents no longer pipe notes through stdin (B-53bf0848). Check: `grep -nE -- '--(note|rationale) -([^a-z]|$)'` over review-finder.md, plan-executor.md, review-sweeper.md and review-verifier.md in `claude/agents/` prints nothing (today: 4 hits).
- forward: glimpse's export-v1 bridge is documented (B-cd31ec02). Check: `grep -qi glimpse claude/skills/review-v2-engine/references/triage-and-close.md` exits 0 (today: exit 1).
- forward: the finder note fits the 600-character cap (B-de835331). **predicted, unverified**: it needs a live `/review-v2` run; read the cells' `note` lengths with `tomlctl review list` after the first run.
- forward: payload inlining is gone. Check: `grep -n '$(cat' claude/agents/review-surveyor.md claude/skills/review-v2-engine/references/compose.md` prints nothing (today: 2 hits).
- forward: the survey passes no relative date to git. Check: `grep -n 'days ago' tomlctl/src/review/survey.rs` prints nothing (today: 2 hits).
- forward: the context-free agent exists. Check: `grep -q '^omitClaudeMd: true' claude/agents/review-finder-cf.md` (today: file absent, exit 2; review-sweeper.md is the positive control, exit 0).
- guard: `bash scripts/verify-shared-blocks.sh` passes, so review-finder-cf.md stays byte-identical to review-finder.md on the shared body.
- guard: the full tomlctl suite and both clippy runs pass.

## Verification Commands

```
build: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets -- -D warnings
test: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
test.timeout: 900
test.rerun: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -j 1 -- --exact {ids}
lint: cargo fmt --manifest-path tomlctl/Cargo.toml -- --check
checkpoint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets -- -D warnings
checkpoint: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
checkpoint: cargo clippy --manifest-path glimpse/Cargo.toml --locked
transient: rust-lld: failed to write output.*[Pp]ermission denied
transient: os error 5
success: ! grep -nE 'const (SETTLED|REOPENABLE|COMPANIONS|SYNCABLE|SYNC_COMPANIONS|SUPPRESSING|DECIDED_BY_VERDICT)\b' tomlctl/src/review/*.rs
success: grep -qE '^[[:space:]]*pub\(crate\) status: FindingStatus,' tomlctl/src/review/store.rs
success: ! grep -l '^fn invalid(' tomlctl/src/review/*.rs
success: ! grep -n 'let invalid' tomlctl/src/review/schema_out.rs
success: test "$(grep -hoE 'fn (reopen_retires_the_v1_link|reopened_finding_survives_sync|v1_reopen_syncs_back_to_open|v2_disposition_after_v1_reopen_survives_sync|reexport_after_reopen_mints_a_regression|reexport_after_reopen_of_a_wontfix_row_mints_a_regression|reuse_keeps_both_v2_refs|plan_ndjson_replay_is_unchanged|set_json_reads_an_at_path_file|health_window_above_cap_is_refused|convert_into_open_run|show_for_finder_cf_omits_the_pack)\(' tomlctl/tests/*.rs | sort -u | wc -l)" -eq 12
success: ! grep -nE -- '--(note|rationale) -([^a-z]|$)' claude/agents/review-finder.md claude/agents/plan-executor.md claude/agents/review-sweeper.md claude/agents/review-verifier.md
success: grep -qi glimpse claude/skills/review-v2-engine/references/triage-and-close.md
success: ! grep -n '$(cat' claude/agents/review-surveyor.md claude/skills/review-v2-engine/references/compose.md
success: ! grep -n 'days ago' tomlctl/src/review/survey.rs
success: grep -q '^omitClaudeMd: true' claude/agents/review-finder-cf.md
success: bash scripts/verify-shared-blocks.sh
success: cargo clippy --manifest-path glimpse/Cargo.toml --locked
```

Phase 3 also runs `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`, because `claude/**/*.md` changes. That covers the carrier and flag-table gates, and the full nextest run includes it.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 12, 13, 15, 16, 17, 19, 21, 22, 24, 25
- **Max parallel agents**: 6
- **Commit granularity**: per-task

## Tasks

### 1. Add FindingStatus and the companion model to the field catalogue [M]
- **Files**: `tomlctl/src/review/fields.rs`, `tomlctl/src/review/mod.rs`
- **Depends on**: —
- **Backlog**: B-8caf86b0, refs B-faca904c, refs B-9e96ce7c, refs B-c2c975d1
- **Action**: Add `str_enum!(FindingStatus)`, its `companions()` table, `ALL_COMPANIONS`, `LINK_FIELDS`, the five predicates, the `retired_links` finding field and the run `incomplete` field to fields.rs. Add `FINDER_CF` and `is_finder()`. Add the shared `pub(super) fn invalid(msg)` to mod.rs.
- **Detail**: Follow Approach, "Status owner and companions". `FINDING_STATUSES` becomes `FindingStatus::VALUES`, with the same order and contents. Each predicate's set must equal today's constant exactly:
  - `is_settled` = triage `SETTLED`
  - `is_reopenable` = triage `REOPENABLE`
  - `is_suppressing` = add `SUPPRESSING`
  - `is_syncable` = sync `SYNCABLE`
  - `decided_by_verdict` = verdict `DECIDED_BY_VERDICT`

  Add the unit test `finding_status_predicates_match_the_old_constants` asserting each set. Do not delete the old constants; their modules do that.
  - `retired_links`: a computed `Ty::StrList` on the finding record (visibility `STATUS`, like the disposition companions), with a matching row added to the `MATRIX` table that the lib test `projection_table_matches_matrix` checks for every finding field.
  - `incomplete`: a computed string on the run record, uncapped as `run finish --incomplete` is today. Computed is what makes `review add` refuse a caller-supplied `retired_links` and `review run start` refuse a caller-supplied `incomplete`.
  - `CELL_AGENTS` gains `FINDER_CF = "review-finder-cf"`, and `is_finder(agent)` matches both finder agents.
  - `invalid` keeps the existing body: `tagged_err(ErrorKind::Validation, None, msg)`.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(finding_status_predicates_match_the_old_constants)'` passes (today: exit 4, no test matches). Falsifier: dropping `refuted` from `is_reopenable` turns it red.
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(projection_table_matches_matrix)'` and `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_vocabulary --test review_catalogue` pass. Falsifier: omitting the `retired_links` `MATRIX` row turns the first red.

### 2. Add the Finding status accessor and the single status-write helper [M]
- **Files**: `tomlctl/src/review/store.rs`
- **Depends on**: 1
- **Backlog**: refs B-faca904c
- **Action**: Add the `Finding` methods `status() -> FindingStatus`, `set_status(FindingStatus, &[(&str, TomlValue)])`, `retire_link()`, `is_retired(ledger, v1_id)` and `unretire_link(ledger, v1_id)`. Migrate store.rs's own status-literal comparisons onto them.
- **Detail**: Follow Approach, "Status owner and companions" and "Retired links".
  - `status()` parses the stored string. Rows are validated on load (`finding_from_toml` checks `status` against the catalogue values), so a parse failure is unreachable; `expect` with that reason.
  - `set_status`: when the status changes or companions are given, remove every `ALL_COMPANIONS` key, then insert the given ones. A same-status call with no companions changes nothing.
  - `retire_link` pushes `"<export_ledger>#<exported_as>"` onto `retired_links` (creating it), then removes both link fields. It is a no-op without `exported_as`.
  - `is_retired` tests membership; `unretire_link` removes that entry (dropping the field when it empties).
  - `Finding.status` stays `String` here; task 10 flips it.

  Add lib unit tests `set_status_clears_stale_companions`, `set_status_keeps_companions_on_a_same_status_write` and `retire_link_moves_the_link_to_retired_links`.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(set_status_) | test(retire_link_)'` passes and reports 3 tests run (today: exit 4, no test matches). Falsifier: leaving `reopen_reason` in place when `set_status(Resolved, …)` runs turns `set_status_clears_stale_companions` red; clearing on a same-status write turns `set_status_keeps_companions_on_a_same_status_write` red.

### 3. Migrate triage onto the lifecycle owner and retire v1 links on reopen [M]
- **Files**: `tomlctl/src/review/triage.rs`, `tomlctl/tests/review_triage.rs`
- **Depends on**: 2
- **Backlog**: B-8a4b923f, B-8caf86b0, refs B-faca904c
- **Action**: Replace `SETTLED`, `REOPENABLE` and `COMPANIONS` with the `FindingStatus` predicates, route `dispose` through `Finding::set_status`, call `retire_link()` when `--reopen` moves a finding carrying `exported_as`, delete the local `invalid`, and correct the "retry after the sidecar rename failed" comment.
- **Detail**: Follow Approach, "Status owner and companions" and "Retired links". Refusal texts and their order stay unchanged: the reopen refusal keeps an ordered slice of the reopenable statuses for its message, because filtering `FindingStatus::ALL` yields enum order. The comment fix: io.rs commits the sidecar before the TOML, so a failed write leaves nothing behind. A retry finds the store unchanged, not its own earlier write. Add the test `reopen_retires_the_v1_link`, which:
  1. seeds a finding at `resolved` with `exported_as` and `export_ledger`
  2. runs `review triage --reopen`
  3. asserts the status is `open`, both link fields are gone, and `retired_links == ["<ledger>#R<n>"]`
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_triage -E 'test(reopen_retires_the_v1_link)'` passes (today: absent). Falsifier: skipping `retire_link()` on reopen turns it red.
  - guard: the whole `review_triage` binary passes.

### 4. Migrate sync-v1, skip retired links and carry a v1 reopen back [M]
- **Files**: `tomlctl/src/review/sync.rs`, `tomlctl/tests/review_sync.rs`
- **Depends on**: 3
- **Backlog**: B-8a4b923f, B-1fc81407, B-6601255f, refs B-faca904c
- **Action**:
  - Replace `SYNCABLE` and `SYNC_COMPANIONS` with `is_syncable` and `Finding::set_status`.
  - Skip links the finding has retired.
  - Read v1 `v2_ref` as a string or an array in `linked_rows`.
  - Map a v1 `open` row that carries `reopen_rationale` to v2 `open` with `reopen_reason`.
- **Detail**: Follow Approach, "Status owner and companions" and "Retired links".
  - The skipped entry's reason is `link retired by reopen`.
  - The v1-reopen mapping applies only when the finding's current status is resolved, dismissed or deferred **and** the finding's `by = "sync"` verdict does not already record this `reopen_rationale`. It writes `reopen_reason = SYNC_REASON_PREFIX + rationale` and records the rationale in that `by = "sync"` verdict, which `upsert_verdict` keeps one of per finding.
  - A plain v1 `open` row without `reopen_rationale` is still skipped.
  - Read the kept `resolved` date before calling `set_status`, so a re-sync on a later day does not restamp it.
  - `linked_rows` reads `v2_ref` as a string or an array but still yields one `Linked` per named finding; `stats::v1_links` (task 8) depends on that shape.

  Add four tests:
  - `reopened_finding_survives_sync`: export, sync to resolved, then `triage --reopen`, then sync. Expect the finding open, with the link listed under `skipped`.
  - `v1_reopen_syncs_back_to_open`
  - `v2_disposition_after_v1_reopen_survives_sync`: after the v1 reopen syncs back, dismiss the finding in v2 and sync again. Expect it to stay dismissed.
  - `sync_clears_a_stale_reopen_reason`
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_sync -E 'test(reopen)'` passes and reports 4 tests run (today: exit 4, no test in review_sync matches). Falsifier: removing the retired-link skip turns `reopened_finding_survives_sync` red; dropping the verdict check turns `v2_disposition_after_v1_reopen_survives_sync` red.
  - guard: `fixed_v1_row_resolves_v2_finding` and `sync_is_idempotent` still pass.

### 5. Migrate export-v1, ignore retired links and keep a second link [M]
- **Files**: `tomlctl/src/review/export.rs`, `tomlctl/tests/review_export.rs`
- **Depends on**: 3
- **Backlog**: B-8a4b923f, B-6601255f, B-8caf86b0, refs B-faca904c
- **Action**:
  - Replace the `== "open"` checks with `FindingStatus`.
  - Have `decide` exclude rows the finding has retired from the `v2_ref` match.
  - Match `v2_ref` by membership when it is a string or an array.
  - On `Reuse` of a row whose `v2_ref` names another finding, write an array holding both.
  - Delete the local `invalid`.
- **Detail**: Follow Approach, "Retired links".
  - `Candidate` carries the finding's `retired_links` and the target ledger's repo-relative path, so `decide` can form `<ledger>#<id>`.
  - A retired row still participates in the merge rule, which is what yields `Regression`, except that a retired row whose v1 status is settled (wontfix, wontapply, verified-clean, deferred) never yields `Settled`: it falls through to `Regression` too.
  - A retired row that is open again in v1 yields `Reuse`; `mark_exported` then calls `Finding::unretire_link`, so sync reads that link again.
  - `mark_exported`'s open → exported write goes through `set_status(Exported, &[])`, which clears companions such as `reopen_reason` because the status changes.

  Add four tests:
  - `reexport_after_reopen_mints_a_regression`: export, sync fixed, reopen, re-export. Expect a new v1 id with `related = [old]`.
  - `reexport_after_reopen_of_a_wontfix_row_mints_a_regression`: the same with the v1 row at wontfix (finding synced to dismissed).
  - `reuse_keeps_both_v2_refs`
  - `reuse_of_a_retired_row_restores_the_link`: the retired v1 row reopened in v1, then re-export. Expect `Reuse` and the entry gone from `retired_links`.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_export -E 'test(reexport_after_reopen) | test(reuse_keeps_both) | test(reuse_of_a_retired)'` passes and reports 4 tests run (today: exit 4, no test matches). Falsifier: dropping the retired filter makes `decide` return `Linked`, and keeping the `Settled` branch for retired rows strands the wontfix case; either turns a test red.
  - guard: `export_is_idempotent_on_v2_ref` still passes.

### 6. Migrate digest and verdict onto the lifecycle owner [M]
- **Files**: `tomlctl/src/review/digest.rs`, `tomlctl/src/review/verdict.rs`
- **Depends on**: 2
- **Backlog**: B-8caf86b0, refs B-faca904c
- **Action**:
  - Replace `DECIDED_BY_VERDICT` with `decided_by_verdict` and the status write (verdict.rs around :256) with `set_status`. Rewrite digest's inline status sets (`audit_sample` around digest.rs:223, `live` around :298) as local `matches!` over `FindingStatus` variants with the same members; no predicate equals them.
  - Keep `DISPOSITIONS` as its existing literal display list, unchanged in membership and order (it deliberately omits `resolved` and `export_ledger`), and add a unit test asserting every entry is in `ALL_COMPANIONS` or `LINK_FIELDS`.
  - Delete verdict's local `invalid`.
  - Correct verdict.rs's "retry after the sidecar rename failed" comment as in task 3.
- **Detail**: Follow Approach, "Status owner and companions". The digest output must stay byte-identical. A verdict that leaves a reopened finding `open` is a same-status write, so `set_status` keeps its `reopen_reason`.
- **Acceptance**:
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_digest --test review_verdict` passes.
  - forward: `grep -n 'DECIDED_BY_VERDICT' tomlctl/src/review/verdict.rs` prints nothing (today: 2 hits).

### 7. Migrate add and resolve onto the lifecycle owner [M]
- **Files**: `tomlctl/src/review/add.rs`, `tomlctl/src/review/resolve.rs`, `tomlctl/src/review/schema_out.rs` — its `invalid` closure only
- **Depends on**: 2
- **Backlog**: B-8caf86b0, refs B-faca904c
- **Action**:
  - Replace `SUPPRESSING` (defined in add.rs, imported by resolve.rs) with `is_suppressing`.
  - Route `Outcome::for_target`, `settled_target` and the status writes in `resolve::apply` and add.rs through `FindingStatus` / `set_status`.
  - Delete add's local `invalid`, and replace schema_out.rs's `let invalid = |msg| …` closure with `super::invalid`.
- **Detail**: Today `resolve::apply`'s refuted→open move writes no `reopen_reason`. Keep that, but go through `set_status`, so stale companions are cleared.
- **Acceptance**:
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_add --test review_resolve` passes.
  - forward: `grep -n 'SUPPRESSING' tomlctl/src/review/add.rs tomlctl/src/review/resolve.rs` prints nothing (today: 4 hits).
  - forward: `grep -n 'let invalid' tomlctl/src/review/schema_out.rs` prints nothing (today: 1 hit).

### 8. Migrate stats and matching onto the lifecycle owner [M]
- **Files**: `tomlctl/src/review/stats.rs`, `tomlctl/src/review/matching.rs`, `tomlctl/tests/review_stats.rs`
- **Depends on**: 2
- **Backlog**: B-8caf86b0, refs B-faca904c, refs B-8a4b923f
- **Action**: Replace the finding-status string comparisons in stats.rs and matching.rs (around matching.rs:39) with `FindingStatus`. In `v1_links`, skip a linked row the finding has retired (`Finding::is_retired`), as sync does, so `outcomes` does not count a retired v1 row beside its regression. Delete stats's local `invalid`.
- **Detail**: Follow Approach, "Status owner and companions" and "Retired links". Leave v1 status words (`wontfix`, `fixed`) and verdict words as strings; they are not finding statuses. matching's `{superseded, merged}` in `candidates` matches no predicate: keep it as a local `matches!` over `FindingStatus` variants. The `matching.rs` test fixture's `status: "open".to_string()` initialiser cannot change before the flip; task 10 owns it. Move matching.rs's `.status = "…".to_string()` assignments (around :136 and :138) onto `set_status`. Add the test `outcomes_skip_a_retired_link`, seeding a finding whose `retired_links` names a v1 row that carries its `v2_ref`.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_stats -E 'test(outcomes_skip_a_retired_link)'` passes (today: exit 4, no test matches). Falsifier: removing the `is_retired` skip counts the retired row and turns it red.
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_stats` and `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(review::matching)'` pass.
  - forward: `grep -c '^fn invalid(' tomlctl/src/review/stats.rs` prints 0 (today: 1).

### 9. Migrate runs and record the incomplete reason as a catalogue field [M]
- **Files**: `tomlctl/src/review/runs.rs`, `tomlctl/tests/review_finish.rs`
- **Depends on**: 2
- **Backlog**: B-9e96ce7c, B-8caf86b0, refs B-faca904c
- **Action**: Replace runs.rs's finding-status literals with `FindingStatus` and delete the local `invalid`. Keep `run finish --incomplete` writing `incomplete` into `run.rest`: catalogued run fields such as `counts` live there too, and task 1's computed catalogue entry is the fix for B-9e96ce7c.
- **Detail**: The stored key name stays `incomplete`, so existing stores read unchanged. No `review list` or `review show` projection serves run rows (`show` accepts only finding and cell ids), so the test checks the write and the refusal instead. Add the test `finish_incomplete_is_a_catalogue_field` beside the other `run finish` tests, asserting that `run finish --incomplete` stores `incomplete` and that `review run start` refuses a row supplying `incomplete` as computed.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_finish -E 'test(finish_incomplete_is_a_catalogue_field)'` passes (today: exit 4, no test matches). Falsifier: leaving `incomplete` uncatalogued (or not computed) lets `run start` accept it and turns the test red.
  - guard: the whole `review_finish` binary passes, including `finish_incomplete_records_reason`.

### 10. Type the finding status field as FindingStatus [M]
- **Files**: `tomlctl/src/review/store.rs`, `tomlctl/src/review/matching.rs` — the `tests::finding` fixture's `status` initialiser only, `tomlctl/src/review/fields.rs`, `tomlctl/src/review/query.rs`, `tomlctl/src/review/resolve.rs`
- **Depends on**: 3, 4, 5, 6, 7, 8, 9, 14
- **Backlog**: B-faca904c
- **Action**: Change `Finding.status` from `String` to `FindingStatus`, parse it in `finding_from_toml`, serialise it in `finding_to_toml`, and make `status()` a field read. Change the `matching.rs` test fixture's `status: "open".to_string()` to `FindingStatus::Open`.
- **Detail**: Every caller already goes through the accessor or `set_status` after tasks 3–9 and 14. The one struct-literal initialiser outside store.rs is the matching.rs test fixture, which cannot change before the flip and so belongs to this task. Any other remaining direct string use is a migration gap: fix it in store.rs if it is there, otherwise stop and report the file. Do not add a `From<&str>` shortcut.
- **Acceptance**:
  - forward: `grep -qE '^[[:space:]]*pub\(crate\) status: FindingStatus,' tomlctl/src/review/store.rs` exits 0 (today: exit 1, and still exit 1 after task 2's accessor).
  - guard: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets -- -D warnings` is clean.

### 11. Make review plan --ndjson replay-safe [M]
- **Files**: `tomlctl/src/review/cells.rs`, `tomlctl/tests/review_cells.rs`, `tomlctl/src/review/mod.rs`
- **Depends on**: 1
- **Backlog**: B-e8b56feb, B-8caf86b0
- **Action**: Inside `append`'s `mutate_minting` edit, compare the prepared rows with the run's newest wave. On an exact match whose cells are all `assigned`, return them with `unchanged: true` and mint nothing. Also delete the local `invalid` and correct `cover`'s "retry after the sidecar rename failed" comment.
- **Detail**: Follow Approach, "Write-path fixes".
  - Key: `(run, slice in order, concerns, pass, question, agent)`.
  - Report the newest wave's existing ids in the same envelope shape as a fresh mint, plus `unchanged: true`.
  - A partial overlap, or any sealed cell, mints a new wave as today.

  Add the test `plan_ndjson_replay_is_unchanged`, which runs the same payload twice and asserts one wave and identical ids.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_cells -E 'test(plan_ndjson_replay)'` passes (today: exit 4, no test matches). Falsifier: removing the comparison mints wave 2, which turns it red.
  - guard: the whole `review_cells` binary passes.

### 12. Accept @path in --set-json for every field-flag verb [M]
- **Files**: `tomlctl/src/fields.rs`, `tomlctl/src/io.rs`, `tomlctl/tests/field_flags.rs`
- **Depends on**: —
- **Backlog**: B-bd78e505
- **Action**: In `fields::build` (`tomlctl/src/fields.rs`), read a `--set-json KEY=@<path>` value as JSON from that file. Make io.rs's `at_file_path` `pub(crate)` and reuse it. Update the `--set-json` doc comment in `tomlctl/src/fields.rs` (around :23), which clap prints as help, to name the `KEY=@<path>` form.
- **Detail**:
  - A bare `@` stays refused.
  - A file that fails to parse names the key and the path.
  - `--set` and `--set-file` are unchanged.

  Add the test `set_json_reads_an_at_path_file`, covering one verb that uses field flags (`items add`) and `review run update`.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test field_flags -E 'test(at_path)'` passes (today: exit 4, no test matches). Falsifier: dropping the `@` branch in `fields::build` parses the path as JSON and turns it red.
  - guard: `set_json_keeps_types_and_flags_win_over_base` passes.

### 13. Bound the survey window and pass git an absolute cutoff [M]
- **Files**: `tomlctl/src/review/survey.rs`, `tomlctl/tests/review_survey.rs`
- **Depends on**: 1
- **Backlog**: B-d8d355f4, B-8caf86b0
- **Action**:
  - Refuse a detector `window` above 36,500 days in `Detect::parse`.
  - In `count_in_window` and `commits_per_file`, compute the cutoff with `crate::time::now()` and `SignedDuration::try_from_hours`, and pass `--max-age=<secs>` (no filter when the cutoff is at or before 0).
  - Delete the local `invalid`.
- **Detail**: Follow Approach, "Write-path fixes". `Span::new().days(n)` errors on `Timestamp`. Add two tests:
  - `health_window_above_cap_is_refused`
  - `health_window_reaching_the_epoch_counts_all_history`: a 20,800-day window counts every fixture commit, each made with `commit_dated` at least two days old. The old `--since=20800 days ago` resolves to about half a day before now, so it counted none of them. `Repo::commit` stamps the current time, which the old code also counted, so it does not discriminate.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_survey -E 'test(=health_window_above_cap_is_refused) | test(=health_window_reaching_the_epoch_counts_all_history)'` passes and reports 2 tests run (today: exit 4, no test matches). Falsifier: passing a non-positive cutoff as `--max-age` (dropping the ≤0 guard) makes git return nothing and turns the epoch test red.
  - guard: `health_window_excludes_commits_before_it` still passes.
  - forward: `grep -n 'days ago' tomlctl/src/review/survey.rs` prints nothing (today: 2 hits).

### 14. Serve context-free finder cells without the pack [M]
- **Files**: `tomlctl/src/review/cells.rs`, `tomlctl/src/review/query.rs`, `tomlctl/tests/review_query.rs`
- **Depends on**: 2, 11
- **Backlog**: B-c2c975d1, B-be44a578, B-8caf86b0, refs B-faca904c
- **Action**:
  - Let a cell for the correctness lens's context-free variant name `review-finder-cf`: in `prepare_cell`'s agent check (the `agent` vs `expected_agent` comparison in `tomlctl/src/review/cells.rs`) and in `agent_for`, use `fields::is_finder` wherever finder-only logic applies.
  - Make `query::assignment()` omit `pack` for that agent, keep `baselines`, and add the run's `base`.
  - Migrate query.rs's `!= "superseded"` onto `FindingStatus`.
  - Delete query's local `invalid`.
- **Detail**: Follow Approach, "Context-free finder cell". No field marks a cell as context-free (`claude/commands/review-v2.md`, stage 3 "Store record"). Accept `review-finder-cf` only when `expected_agent` returns `FINDER` and the cell's concerns are exactly `["correctness"]`; every other cell refuses it. Add both tests to `tomlctl/tests/review_query.rs` (the refusal goes through `review plan`, which any integration file can drive):
  - `show_for_finder_cf_omits_the_pack`: `review show <cell> --for finder` on a cf cell has no `pack`, but has `baselines` and the run's `base`.
  - `cf_agent_refused_outside_the_variant`: a `review plan` row naming `review-finder-cf` on a non-correctness cell is refused.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_query -E 'test(finder_cf) | test(cf_agent)'` passes and reports 2 tests run (today: exit 4, no test matches). Falsifier: leaving `pack` in the cf assignment, or accepting the agent on any finder cell, turns a test red.
  - guard: the whole `review_cells` and `review_query` binaries pass.

### 15. Convert a finished run's advisory into a named open run [M]
- **Files**: `tomlctl/src/review/triage.rs`, `tomlctl/src/cli/types/review.rs`, `tomlctl/tests/review_triage.rs`
- **Depends on**: 3, 10
- **Backlog**: B-4a72f1a6
- **Action**: Add `--run <id>` to `review triage --convert`. When the advisory's run is finished and `--run` names an open run, mint the replacement there. Otherwise keep the refusal.
- **Detail**: Follow Approach, "Convert into an open run". This task runs after task 10, so it writes against the typed `Finding.status`.
  - Replacement: `run` and `seen_in` come from `--run`, `cell` from the advisory, status `unverified`, plus the usual `orchestrator` confirmed verdict with its `run` set to the named run.
  - Route through the named run's catalogue and depth (`replacement_row` reads them from the advisory's run today).
  - Refuse a `--run` whose family differs from the advisory's.
  - If the advisory's run is open, `--run` must be absent or name that run.
  - `--run` naming a finished or unknown run is a validation error.
  - Update `convert_refuses_a_finished_run` so it still covers the no-`--run` refusal.

  Add the tests `convert_into_open_run`, asserting the new row's run, its verdict's run, and that `run finish` on the open run then refuses until it is verified; and `convert_refuses_a_foreign_family_run`.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_triage -E 'test(convert_into_open_run) | test(convert_refuses_a_foreign)'` passes and reports 2 tests run (today: exit 4, no test matches). Falsifier: ignoring `--run` keeps the finished-run refusal, which turns `convert_into_open_run` red.
  - guard: the whole `review_triage` binary passes.
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes (the flag-table gate).

### 16. Document --set-json @path in the tomlctl skill [S]
- **Files**: `claude/skills/tomlctl/SKILL.md`, `claude/skills/tomlctl/references/write.md`
- **Depends on**: 12
- **Backlog**: B-bd78e505
- **Action**: State in the Guidance canon's field-flag bullet and in write.md's field-flag table that `--set-json KEY=@<path>` reads JSON from a file staged with the Write tool.
- **Detail**: One sentence each. Point at it as the alternative to inlining `$(cat …)`.
- **Acceptance**: forward: `grep -c 'set-json KEY=@' claude/skills/tomlctl/SKILL.md claude/skills/tomlctl/references/write.md` reports ≥1 per file (today: 0 each).

### 17. Replace $(cat) payload inlining with @path in the surveyor and compose docs [S]
- **Files**: `claude/agents/review-surveyor.md`, `claude/skills/review-v2-engine/references/compose.md`
- **Depends on**: 12
- **Backlog**: B-bd78e505, refs B-c2c975d1
- **Action**: Rewrite `--set-json "signals=$(cat …)"` (review-surveyor.md around :27) and `--set-json "waivers=$(cat …)"` (compose.md around :124) as `--set-json signals=@<path>` / `waivers=@<path>`. Add `review-finder-cf` wherever compose.md enumerates the cell agents (around :58 and :66).
- **Acceptance**: forward: `grep -n '$(cat' claude/agents/review-surveyor.md claude/skills/review-v2-engine/references/compose.md` prints nothing (today: 2 hits).

### 18. Compress the finder coverage note and use the --*-file twins [M]
- **Files**: `claude/agents/review-finder.md`, `claude/skills/review-v2-method/SKILL.md`
- **Depends on**: —
- **Backlog**: B-de835331, B-53bf0848, refs B-c2c975d1
- **Action**: Rewrite review-finder.md's seal step (around :39 and :47) to the terse note shape, and drop the "line per clean probe answer" ask in review-v2-method/SKILL.md (around :139). Switch both files' `--note -` stdin instructions to `--note-file <path>`. Add `review-finder-cf` to the method skill's agent lists (around :3 and :211).
- **Detail**: Follow Approach, "Finder note" and "Context-free finder cell". Have the body state the 600-character cap. In review-v2-method/SKILL.md, state the precedence rule: where a bound lens, method or check asks for one note line per sweep, probe, task or pair, a count plus the failing items satisfies it within the cap. Add the context-free branch to review-finder.md: when `review show <cell> --for finder` returns no `pack`, read the slice as `git diff <base>` using the assignment's `base`, and skip the prior-findings read (`tomlctl review list --in … --for finder`). Keep review-finder.md's body free of anything that only makes sense with CLAUDE.md loaded, because task 20 copies it verbatim into the cf agent.
- **Acceptance**:
  - forward: `grep -nE -- '--note -[^a-z]' claude/agents/review-finder.md` prints nothing (today: 1 hit).
  - forward: `grep -c '600' claude/agents/review-finder.md` ≥ 1 (today: 0).

### 19. Switch the remaining v2 agents to the --*-file twins [M]
- **Files**: `claude/agents/plan-executor.md`, `claude/agents/review-sweeper.md`, `claude/agents/review-verifier.md`
- **Depends on**: —
- **Backlog**: B-53bf0848, refs B-de835331
- **Action**: Replace `--note -` (plan-executor.md around :42, review-sweeper.md around :38) and `--rationale -` (review-verifier.md around :40) stdin instructions with `--note-file <path>` / `--rationale-file <path>`, naming the same staged payload files. In review-sweeper.md's note step, ask for counts plus the escalated and failing items rather than a line per probe, and state the 600-character cap.
- **Acceptance**: forward: `grep -nE -- '--(note|rationale) -[^a-z]' claude/agents/plan-executor.md claude/agents/review-sweeper.md claude/agents/review-verifier.md` prints nothing (today: 3 hits).

### 20. Add the review-finder-cf agent under body parity [M]
- **Files**: `claude/agents/review-finder-cf.md` (new), `claude/agents/review-finder.md`, `scripts/shared-blocks.toml`
- **Depends on**: 18
- **Backlog**: B-c2c975d1
- **Action**: Create review-finder-cf.md with review-finder's frontmatter plus `omitClaudeMd: true`, a description saying it serves context-free cells, and the same body. Wrap review-finder.md's body in a new parity block. Register the new file in that block and in the `research-delivering` and `review-v2-store-writer` blocks. Update the "six v2 review agents" wording in `scripts/shared-blocks.toml` (around :95) to seven.
- **Detail**: Follow Approach, "Context-free finder cell". Wrap the whole body after the frontmatter in one new parity block. Nesting is supported: both `scripts/verify-shared-blocks.sh` and `tomlctl/src/blocks.rs` extract a block by its own markers only, so the two existing blocks inside it are ordinary content.
- **Acceptance**:
  - forward: `grep -q '^omitClaudeMd: true' claude/agents/review-finder-cf.md` (today: file absent, exit 2; review-sweeper.md is the positive control).
  - guard: `bash scripts/verify-shared-blocks.sh` exits 0. Falsifier: editing one word in the cf body makes it exit non-zero.

### 21. Run the context-free cell through the Workflow [M]
- **Files**: `claude/workflows/review-v2.js`, `claude/commands/review-v2.md`, `claude/skills/review-v2-lenses/lenses/correctness.md`
- **Depends on**: 14, 20
- **Backlog**: B-c2c975d1, B-be44a578
- **Action**:
  - Add `review-finder-cf` to review-v2.js's `CELL_AGENTS`, omit the `pack:` preamble line for it, and update the `meta.phases` detail (around :6) that lists the cell agents.
  - In review-v2.md, compose the context-free cell with that agent (stage 3) and delete stage 4's hand-dispatch and its `check-<wave>-cf` batch.
  - Update correctness.md's `### Context-free variant` to name the agent.
- **Detail**: Follow Approach, "Context-free finder cell". The cf cell's findings route like any finder cell's, so no special checker batch is needed.
- **Acceptance**:
  - forward: `grep -q 'review-finder-cf' claude/workflows/review-v2.js` (today: exit 1).
  - forward: `grep -c 'check-<wave>-cf' claude/commands/review-v2.md` prints 0 (today: ≥1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes.

### 22. Name review-finder-cf in the agent lists [M]
- **Files**: `CLAUDE.md`, `claude/skills/review-v2-lenses/SKILL.md`, `claude/skills/review-v2-engine/references/agent-waves.md`
- **Depends on**: 20
- **Backlog**: refs B-c2c975d1, refs B-d8d355f4
- **Action**: Add `review-finder-cf` where each file lists the v2 agents (CLAUDE.md's Review v2 "Agents" bullet; lenses SKILL.md around :44; agent-waves.md around :55), noting that it is Opus without CLAUDE.md. In agent-waves.md also make the preamble's `pack:` line (around :47) conditional: left out for a `review-finder-cf` cell, in both the wave cell prompt and the mop-up re-dispatch, matching review-v2.js. In lenses SKILL.md, update the survey-window text (around :157, `git log --since`) to the absolute `--max-age` cutoff and the 36,500-day cap.
- **Acceptance**: forward: `grep -c 'review-finder-cf' CLAUDE.md claude/skills/review-v2-lenses/SKILL.md claude/skills/review-v2-engine/references/agent-waves.md` reports ≥1 per file (today: 0 each).

### 23. Document the lifecycle, link, replay and convert changes in the review reference [M]
- **Files**: `claude/skills/tomlctl/references/review.md`, `claude/skills/review-v2-engine/references/triage-and-close.md`
- **Depends on**: 4, 5, 9, 11, 12, 14, 15
- **Backlog**: B-cd31ec02, B-4a72f1a6, refs B-8a4b923f, refs B-6601255f, refs B-e8b56feb
- **Action**: Update `claude/skills/tomlctl/references/review.md` for:
  - `retired_links` and reopen retiring the link
  - sync skipping retired links, carrying a v1 reopen back and clearing stale companions
  - `v2_ref` as a string or an array
  - `plan --ndjson` replay returning `unchanged`
  - `triage --convert --run`
  - the run `incomplete` field
  - `review-finder-cf` in the agent list (around :58), the omitted pack and the added `base`
  - `--set-json KEY=@<path>` in the `run start` / `run update` flag rows (around :161 and :177)

  In triage-and-close.md, state the convert-into-open-run rule for advisories revisited after their run finished, and that glimpse shows v2 findings only once exported (the export-v1 bridge).
- **Acceptance**:
  - forward: `grep -c 'retired_links' claude/skills/tomlctl/references/review.md` ≥ 1 (today: 0).
  - forward: `grep -ci 'glimpse' claude/skills/review-v2-engine/references/triage-and-close.md` ≥ 1 (today: 0).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes.

### 24. Refresh the stale agent count, README rows and sidecar story [M]
- **Files**: `claude/skills/review-v2-engine/SKILL.md`, `tomlctl/README.md`, `claude/skills/flow-contract-task-store/SKILL.md`
- **Depends on**: 12, 15, 20
- **Backlog**: refs B-c2c975d1, refs B-bd78e505, refs B-4a72f1a6, refs B-e8b56feb
- **Action**:
  - In `claude/skills/review-v2-engine/SKILL.md` (around :94), change "each of the six v2 agents" to seven and add `review-finder-cf` to its models list.
  - In `tomlctl/README.md`, add `--run` to the `review triage --convert` line (around :97) and the `KEY=@<path>` form to the `field_flags` row (around :479).
  - In `claude/skills/flow-contract-task-store/SKILL.md` §14 (around :414), correct the "sidecar left stale" story: io.rs commits the sidecar before the TOML, so a failed write leaves nothing behind, as tasks 3, 6 and 11 correct in the code comments.
- **Acceptance**:
  - forward: `grep -c 'review-finder-cf' claude/skills/review-v2-engine/SKILL.md` ≥ 1 (today: 0).
  - forward: `grep -c -- '--convert.*--run' tomlctl/README.md` ≥ 1 (today: 0).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes.

### 25. Document the survey health window cap in the review reference [S]
- **Files**: `claude/skills/tomlctl/references/review.md`
- **Depends on**: 13, 23
- **Action**: In claude/skills/tomlctl/references/review.md, section `tomlctl review survey` (the paragraph beginning "Evaluates every `stack`, `diff` and `health` detector"), add one sentence: a health detector whose `window` exceeds 36,500 days is recorded under `unknown` with the reason the code emits (read `Detect::parse` / `MAX_WINDOW_DAYS` in tomlctl/src/review/survey.rs for the exact wording), and the window is passed to git as an absolute `--max-age=<epoch secs>` cutoff, with no filter when the cutoff reaches the epoch. Edit nothing else.
- **Acceptance**: forward: `grep -c 36,500 claude/skills/tomlctl/references/review.md` (or `36500`) reports at least 1 (today: 0). guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 12, 13, 15 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15. Every review module migrated, Finding.status typed, and the reopen/sync, replay, @path, survey and convert fixes in the crate; no earlier cut, because tasks 1 and 2's helpers have no non-test caller until tasks 3–9 and 14 (Rust milestone)

— CHECKPOINT B after tasks 16, 17, 19, 21, 22, 24, 25 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25. Agents, workflow and docs (prose milestone)

## Risks
- Scope is about 50 files, above the ~25-file guideline. That was accepted because you asked for these items together. Mitigation: two checkpoints, ≤3 files per task, and Rust (A) and prose (B) kept in separate checkpoints. Each checkpoint must pass the full suite before it commits. Checkpoint A carries 15 tasks; a cut after tasks 1–2, or after the schema tasks 4–5, would fail clippy's `dead_code` under `-D warnings` because the new helpers have no non-test caller yet.
- The accessor-then-flip migration leaves the crate compiling at every task, but parallel tasks 3–9 share the working tree. Mitigation: build discipline. Each delegate runs clippy only and reports breakage in a file it does not own, and the orchestrator names the in-flight sibling files.
- `retired_links` and the array `v2_ref` change the store and v1 row schema. Older tomlctl binaries reading a store with `retired_links` keep it in `rest`, which is harmless. An older binary reading an array `v2_ref` skips that row in `linked_rows`, and an older `export-v1` that reuses the row overwrites the array with one string, dropping the other finding's link. Mitigation: `cargo install --path tomlctl` After Merge, and the array is written only on the rare second link.
- The cf agent loses CLAUDE.md, rules and memory, including the documentation and comment rules. Mitigation: those rules matter for edits, and finders do not edit source; its preloaded skills still load.
- Body parity between the two finder files needs block markers inside existing blocks. Mitigation: none needed; `scripts/verify-shared-blocks.sh` and `tomlctl/src/blocks.rs` both extract a block by its own markers only, so task 20's outer block may enclose the two existing blocks.
- The cf body is shared with review-finder.md, so the context-free branch (diff against `base`, skip prior findings) lives in both and is keyed on the assignment carrying no pack. Mitigation: task 14's `show_for_finder_cf_omits_the_pack` pins what the assignment carries.
- `set_status` changes companion handling at sites that assigned the status string directly: an open → exported export now clears companions. Mitigation: same-status writes keep companions, and each migrating task's guard binary covers its writes.

## After Merge
- `cargo install --path tomlctl` (new flags, the `retired_links` field, the replay behaviour). Also run it at checkpoint A: `claude/` is live-linked, so from task 21 the carrier composes `review-finder-cf` cells that an older binary refuses. Do not run `/review-v2` between task 21 and the deploy and restart below.
- `pwsh -File scripts/deploy-claude.ps1` to link `claude/agents/review-finder-cf.md` into `~/.claude/agents/`, then restart Claude Code sessions so the new agent registers.
- Resync the saved Workflow `review-v2-pipeline` (it is linked by deploy-claude).

## Exploration Notes

### Finding lifecycle (B-faca904c, B-8a4b923f, B-1fc81407)
- `fields.rs` `FINDING_STATUSES` (12): unverified, superseded, open, refuted, merged, parked, dismissed, deferred, backlogged, converted, resolved, exported. `str_enum!` (fields.rs:17-50) derives Ord/Hash, generates ALL/VALUES/as_str/parse; `pub(crate) use str_enum`. catalogue.rs already has a `Status` enum (experimental/stable/deprecated) — new type needs another name (`FindingStatus`).
- `store.rs` `Finding.status: String` (typed field, ~169); all 12 companions live in `rest: Table` (fields.rs:521-532 lists them: merged_into, park_reason, dismiss_reason, defer_reason, defer_trigger, reopen_reason, backlog_ref, converted_to, resolution, resolved(date), exported_as, export_ledger). Read via `finding_from_toml` (`listed(t,"status",…,FINDING_STATUSES)`), write via `finding_to_toml`. `store::mutate` / `mutate_minting` own writes.
- Subsets: triage SETTLED {superseded,merged,converted,exported}; REOPENABLE {dismissed,deferred,parked,backlogged,refuted,resolved}; COMPANIONS (8, omits merged_into, converted_to, exported_as, export_ledger); sync SYNCABLE {open,exported,resolved,dismissed,deferred}; SYNC_COMPANIONS {resolution,resolved,dismiss_reason,defer_reason,defer_trigger}; digest DISPOSITIONS (display; omits resolved, export_ledger); add SUPPRESSING {dismissed,deferred,parked,backlogged,converted} (resolve imports); verdict DECIDED_BY_VERDICT {unverified,open,refuted}. Inline: digest.rs:223/298, matching.rs:39, resolve.rs:56-65/226/240, export.rs:251/316/810, query.rs:62.
- Status-word literals per file: sync 25, triage 22, resolve 20, fields 17, digest 16, add 12, export 11, stats 11, verdict 11, runs 2, query 1 (raw; includes v1 words, verdict "refuted", date key).
- Status writes: triage::dispose (clears COMPANIONS), sync::set_status (clears SYNC_COMPANIONS only), export::mark_exported (adds exported_as/export_ledger), verdict.rs:256, resolve::apply (279/286/289, clears nothing; refuted→open writes no reopen_reason), add.rs:169 (superseded), new rows add.rs:564 / triage::finish_row.
- Defect path: sync resolves an exported finding, keeping exported_as/export_ledger (asserted by review_sync `fixed_v1_row_resolves_v2_finding`); `triage --reopen` (resolved ∈ REOPENABLE) keeps the link; next `sync-v1` (stage 0 of every run) matches via `linked_rows` (v2_ref `<store>#F<n>`) and, because open ∈ SYNCABLE, re-resolves. Re-export: `export::decide` (~723) returns Linked on existing v2_ref. Same for sync-dismissed/deferred. `stats::v1_links` also reads `linked_rows`. No history field on findings; verdict log has `by="sync"` rows.
- Tests: review_triage (`triage_honours_expect_status`, `triage_refuses_a_settled_finding`), review_sync (7 tests incl. `sync_is_idempotent`), review_export (`export_is_idempotent_on_v2_ref`, `export_regression_on_fixed_v1_match`), review_resolve. **No reopen-then-sync / reopen-then-re-export test.**
- Outside review/: only cli/types/review.rs:416 help string lists statuses; glimpse reads no v2 status.

### Idempotency / ergonomics / survey (B-e8b56feb, B-bd78e505, B-d8d355f4, B-de835331)
- `review plan --ndjson` → `cells.rs::append`: `prepare_cell` per row, then `store::mutate_minting`; wave = max(run's waves)+1, fresh `C<n>` ids — no dedupe; any replay duplicates. Natural key `(run, slice-in-order, concerns, pass, question, agent)`; repeat cells differ by reversed slice + pass=2; follow-ups need narrower slice or question (compose.md §7). Idempotency precedents: cover `sealed_as` (unchanged:true / error on mismatch), verdict/sync/resolve trial-clone compare, triage `unchanged` ids, `io::write_doc_unless_unchanged`.
- io.rs `write_serialized_with_sidecar`: sidecar committed before TOML; commit retries 3×50 ms on PermissionDenied; on failure nothing lands (TOML failure resets sidecar). So the os-error-5 path cannot duplicate under the current binary; comments in cover/verdict/triage ("a retry after the sidecar rename failed finds its own…") describe a landed-then-failed order that no longer exists. Duplication needs a replay of a call that succeeded (output lost / agent retry).
- `review run update` → `runs.rs::update` → `flag_fields` → `crate::fields::build`; `--set-json` is `serde_json::from_str` (no @path). Shared helper `io::read_json_arg(&str)` handles `-` / `@path` / literal (`at_file_path` private). `$(cat)` sites: claude/agents/review-surveyor.md:27 (signals), claude/skills/review-v2-engine/references/compose.md:124 (waivers).
- survey.rs `Detect::parse` (~286) accepts any window > 0; `count_in_window` (~731) and `commits_per_file` (~799) pass `--since={days} days ago`. git 2.55 probe: 20500→all 1110 commits, 21000→15, 30000→0, 50000→1110, i64::MAX→0 (wraps past the epoch ≈20,739 days). Empty log settles False silently (never Unknown). Vocabulary windows: 90/90/90/180 (claude/skills/review-v2-lenses/vocabulary.toml). Tests: review_survey `health_window_excludes_commits_before_it` etc.; none for large window/empty log.
- Cell note cap 600 (fields.rs `field(Cell,"note").cap(600)`). review-finder.md:47 asks for files read in full, checks run, ruled-out candidates + why, reason for thin/uncovered; line 39 a line per passing check; review-v2-method/SKILL.md:139 a line per clean probe answer. Estimated need 900-1,200 chars for a typical cell.

### Decision items (B-c2c975d1 + B-be44a578, B-cd31ec02, B-4a72f1a6)
- Agents: review-finder/review-verifier/plan-executor (opus) lack `omitClaudeMd`; checker/surveyor/sweeper (haiku) set it. Context-free variant: claude/skills/review-v2-lenses/lenses/correctness.md `### Context-free variant`. review-v2.md stage 3 composes it with agent review-finder; stage 4 hand-dispatches it outside the Workflow with `context: none`, no pack, checker batch `check-<wave>-cf`.
- review-v2.js: `CELL_AGENTS=['review-finder','review-sweeper','plan-executor']`; `runCell` uses `cell.agentType`; preamble always passes `pack:`. fields.rs FINDER/SWEEPER/EXECUTOR/CELL_AGENTS validate cell.agent (fields.rs:387); compared at cells.rs:531/964, digest.rs:252, verdict.rs:223/270. query.rs `assignment()` always includes pack (contradicts review-finder.md:29 "no pack is deliberate").
- New agent costs: deploy-claude.ps1 rerun; scripts/shared-blocks.toml blocks `research-delivering` + `review-v2-store-writer`; prose lists in CLAUDE.md:37, tomlctl references/review.md:58, review-v2-lenses/SKILL.md:44, review-v2-method/SKILL.md:3,211, engine references/agent-waves.md:55, compose.md:58,66. lint.rs lints every agent file automatically.
- Glimpse: `ledgers::LedgerKind {Review,Optimise,PlanReview}` + `scopes`/`read`/`write_guarded` assume v1 `items` rows; lib.rs exports ledger_* facade; `mod review` private. A ReviewV2 kind needs a new read projection (query::View), a write facade over triage (triage resolves root from process; needs a root-taking variant), and glimpse row/surface/actions. Today v2 reaches glimpse only via export-v1.
- Convert-on-finished-run: triage.rs `replacement_row` (~548) refuses; `finish_row` stamps run/cell/seen_in from the advisory, status unverified + orchestrator confirmed verdict. Close ordering in engine references/triage-and-close.md already puts triage (stage 7) before run finish (stage 8.5). Failure case: advisory triaged after its run finished (parked item revisited later). `run finish` gaps: unverified findings, unresolved same_as, unwaived concerns/files, unsealed cells.

### Backlog
Live rows under the plan's directories (all `open`): B-8caf86b0, B-615a500d, B-e4c1a22a, B-be44a578, B-9e96ce7c, B-1fc81407, B-e8b56feb, B-6601255f, B-53bf0848, B-de835331, B-e922f5cb, B-cd31ec02, B-bd78e505, B-4a72f1a6, B-ef306556, B-d8d355f4, B-771b57e7, B-8a4b923f, B-faca904c, B-c2c975d1. Requested by the user: B-faca904c, B-8a4b923f, B-1fc81407, B-e8b56feb, B-bd78e505, B-d8d355f4, B-de835331, B-c2c975d1, B-be44a578, B-cd31ec02, B-4a72f1a6.

### Verification commands
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`
- `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast`
- `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check`
- `cargo clippy --manifest-path glimpse/Cargo.toml --locked` (tomlctl lib changes)
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` (claude/**.md edits)
- Review test binaries: review_add, review_catalogue, review_cells, review_digest, review_export, review_finish, review_query, review_resolve, review_runs, review_stats, review_survey, review_sync, review_triage, review_verdict, review_vocabulary.

## Research Notes

### git log time windows (git 2.55.0.windows.5)
Searched: git `date.c` v2.55.0 (raw.githubusercontent.com, fetched 2026-10-13); local `git rev-parse --since=…` probes (re-run by orchestrator: `25000 days ago` → `--max-age=3926849661`, a future time; `2020-01-01T00:00:00Z` → `--max-age=1577836800`).
- Relative `--since` goes through approxidate, which never errors: past ~20,700 days it returns arbitrary cutoffs (now, future, or tiny); unparseable input resolves to now. `tm_to_time_t` handles only 1970-2099; day×seconds multiply overflows `int` past ~24,855 days. (high)
- Safe forms: `--max-age=<epoch secs>` is parsed strictly (`--max-age=abc` → fatal rc 128); `--max-age=0` keeps all; negative values return nothing, so never pass one. Full ISO with `Z` also parses exactly; bare `YYYY-MM-DD` takes the current local time of day; `@epoch` needs a timezone. (high)
- Impact on plan: compute the cutoff in Rust and pass `--max-age=<secs>`; when the cutoff is ≤ 0 pass no filter (the window covers all history). Cap `window` at parse time (proposed 36,500 days). An empty log under a valid window genuinely means "no commits in range".

### jiff 0.2.37 (tomlctl/Cargo.lock)
Searched: installed source `jiff-0.2.37/src` (timestamp.rs:1538 `checked_sub`, signed_duration.rs:1733 `try_from_hours`), fetched 2026-10-13.
- `Timestamp::checked_sub(Span::new().days(n))` errors (calendar units not allowed on Timestamp); use `SignedDuration::try_from_hours(days.checked_mul(24)?)` and `Timestamp::checked_sub(SignedDuration)`. Use `crate::time::now()` (time.rs:37) so the test clock override applies. No new dependency. (high)

### Claude Code subagent context (2.1.296, native binary)
Searched: installed binary `~/.local/share/claude/versions/2.1.296` (grep of all 28 `omitClaudeMd` sites); https://code.claude.com/docs/en/sub-agents (fetched 2026-10-13).
- `omitClaudeMd: true` drops user and project CLAUDE.md, `.claude/rules/*.md`, CLAUDE.local.md, auto-memory MEMORY.md and nested CLAUDE.md; only managed-policy files remain. Preloaded `skills:` still load in full. (high)
- No per-call override: the Agent tool takes only prompt/description/subagent_type/model/effort/isolation/name/cwd/run_in_background; Workflow `agent()` opts are `{label, phase, schema, model, effort, isolation, agentType}`. A context-free variant therefore needs a second agent definition selected by `agentType`. (high)
- Impact on plan: a context-free finder = a new agent file with `omitClaudeMd: true`, body kept identical to review-finder via a shared-block parity entry; Workflow picks it via `cell.agentType`; deploy-claude.ps1 rerun.
