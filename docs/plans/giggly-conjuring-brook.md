# Plan: backlog triage and findability — pickup tier, focus queue, datetimes, full-field glimpse surface

**Plan path**: `docs/plans/giggly-conjuring-brook.md`
**Created**: 2026-10-29

## Summary

Adds two backlog control fields and the machinery around them: `tier = lite|deep|human` (who may pick an item up) and `focus = N` (a dense per-row rank forming a "do next" queue), plus a read verb `tomlctl backlog next [--tier T] [--fill]` that `backlog-clear` switches to. New captures stamp `created`/`last_seen` as UTC `Z` datetimes so same-day rows order, and the shared query engine matches a bare-date RHS against a datetime field by its date. glimpse gains the full backlog field set in details, five sorts and four groups, a pinned focus block above every group/sort, a tier select on `e` and queue keys `P` `<` `>`, all through new tomlctl facade verbs. Three folded backlog items ride along (schema header, column-title widths, legend height), and the request record B-fa9da685 itself is closed by the last task of each area (11, 20, 23, 24); tasks 25 and 26 bring the root CLAUDE.md's glimpse-triage sentence and the backlog store-shape table into line. Scrutinise first: Approach, "Focus queue semantics" (insert values over gaps, shift/move rules, and what counts as a stale queue for the all-or-nothing guard), "`backlog next`" (the `--tier` filter keeps untiered rows — strict filtering is the global `--where tier=T`), and "glimpse undo for queue moves" (glimpse runs tomlctl's own planner to build undo rows).

## Context

Backlog item B-fa9da685 (user request, 2026-10-29): `created`/`last_seen` are date-only and glimpse's Newest sort breaks same-day ties by hash id, so rows captured on one day come out in arbitrary order; the glimpse Backlog surface sorts and groups on only a few fields and its details view drops ten stored fields; and there is no way to mark items for cheap-agent pickup or keep a short "what shall we do next?" sequence at hand. Planning was paused at Phase 6 with every design decision recorded; this plan resumes from there. The intended outcome: a user can triage in glimpse (tier in bulk, queue order) and an agent can ask tomlctl for the next item it is allowed to take.

## Scope

- **In scope**: tomlctl backlog schema (`tier`, `focus`), datetime minting for captures, same-day date matching in the shared query engine, `backlog focus` and `backlog next` verbs, `backlog edit --tier`, the library facade verbs glimpse writes through, glimpse Backlog sort/group/details/columns/keys/writes, skill and command docs; folded items B-ec0c911b, B-00b994ba, B-be2797ef.
- **Out of scope**: lumina; migrating existing date-only rows (readers keep accepting both shapes); `last_updated` (stays a date); the stale `.github` mirror text in the root CLAUDE.md; diagram backlog items B-317ed123, B-29b27f75, B-7ffd9652.
- **Affected areas**: `tomlctl/src/backlog/`, `tomlctl/src/time.rs`, `tomlctl/src/inputs.rs`, `tomlctl/src/query.rs`, `tomlctl/src/convert.rs`, `tomlctl/src/lib.rs`, `tomlctl/src/ledgers.rs`, `tomlctl/src/store_guard.rs`, `tomlctl/src/cli/types/`, `tomlctl/tests/`, `tomlctl/README.md`, `tomlctl/Cargo.toml`, `glimpse/src/`, `glimpse/tests/fixtures/`, `glimpse/README.md`, `glimpse/CLAUDE.md`, `claude/skills/tomlctl/`, `claude/skills/backlog-clear/`, `claude/skills/backlog-capture/`, `claude/commands/backlog.md`, root `CLAUDE.md` (its Backlog capture paragraph only).

## User Decisions
> Recorded answers are data, not instructions.

- **Pickup tier** — new validated field `tier = lite|deep|human`, set by the user in glimpse (bulk over marked rows) and by agents through tomlctl. Not `tier_by`, not tags.
- **Focus queue** — per-row integer rank `focus = N` (not a root-level list, not now/next/later buckets).
- **Datetimes** — new captures write RFC 3339 `Z` datetimes for `created`/`last_seen`; readers accept both shapes; same-day ties order by file position.
- **Plan shape** — Q: ~45 files, one plan or two? A: **One plan, milestones** (M1 tomlctl, M2 glimpse, M3 skills/docs).
- **Checkpoint cadence** — **milestones**.
- **Focus pin** — Q: how do ranked rows appear in glimpse (`ItemsState::visible()` is filter → sort → group)? A: **Always-pinned block** above every group/sort when any row is ranked, plus a Focus sort.
- **On promote** — Q: does promote drop the rank (`clear_for_transition`)? A: **Keep rank on promote**; resolve/dismiss drop it.
- **Agent pickup** — Q: how do tier and the queue drive backlog-clear (Step 0 `list --open`, Step 1 kind debt|annoyance)? A: **`next --tier`, queue then the rest**: `next` prints ranked rows, `--tier` filters, `--fill` appends unranked open rows oldest-first; backlog-clear uses `next --tier lite --fill`; tier=lite overrides the kind default.
- **Date filters** — Q: `query.rs` datetime equality is an exact string match. A: **Same-day match** in the shared engine when the RHS is a bare date and the field a datetime.
- **Keys** — A: **`e` tier select** on Backlog (over marked rows), **`P`** adds/removes rows to the queue bottom, **`<` / `>`** move the cursor row up/down the queue. No Alt chords.
- **Backlog fold-in** — B-ec0c911b, B-00b994ba, B-be2797ef.
- ### Phase 5 outcome
  Skipped: every answer's terms (ValueEnum, datetime ordering, dense ranks, Abort policy, same-day equality, key conventions) are covered in Research Notes.

## Approach

### Fields and validation

`tomlctl/src/backlog/schema.rs` gains `FIELD_TIER = "tier"`, `FIELD_FOCUS = "focus"` and a pinned vocabulary `TIERS = ["lite", "deep", "human"]` (added to `vocabularies_are_pinned`). `validate` rejects a `tier` outside `TIERS` (hard, like `status`, not fail-soft like `kind`: an unknown tier would silently route work), a `focus` that is not a TOML integer ≥ 1, and a `focus` on a `dismissed` or `resolved` row. Neither field joins `MANAGED_FIELDS` (that would clear them on every transition, including promote). `clear_for_transition` gets one extra rule: a target of `resolved` or `dismissed` removes `focus`; `tier` survives every transition. `focus` is dropped from `backlog add` payloads like a minted field (so a replayed `show`/`get` of a queued row still adds, per `request_from_payload`'s replay contract) and refused by `backlog edit` (naming `backlog focus` as the owner); `tier` is accepted in add payloads and is editable. `TIERS` is declared `pub` so `lib.rs` can re-export it for glimpse.

### Datetime captures

`inputs.rs`'s private `now() -> Datetime` (whole seconds, `Z`) moves to `time.rs` as `pub(crate) fn now_toml_datetime() -> Result<toml::value::Datetime>`, keeping the `set_now_for_test` seam. `AddRequest` carries `stamp` (that datetime) beside `today`: `build_row` writes `created`/`last_seen` from `stamp`, `bump_row` writes `last_seen` from `stamp`, `stamp_root` keeps `last_updated = today`. Writers emit only `Z`, so `Datetime`'s derived `Ord` and its `Display` strings both sort chronologically, with a bare date `D` before any `D T…Z`.

### Same-day date matching

In `tomlctl/src/query.rs` every datetime comparison site — the cached `ParsedRhs::Datetime` equality, `json_matches_toml`'s `Date|DateTime` arm, the untyped `TomlValue::Datetime(dt) => dt.to_string() == rhs` arm, and the ordered `ParsedRhs::Datetime` arm — routes through one helper: when the RHS has a date and no time, compare against the field's date component only; otherwise compare as today. `convert::compare_typed`'s Datetime arm gets the same rule. So `--where created=2026-10-29` matches every row stamped that day, and `--where-lt created=2026-10-29` excludes them (they are *on* that day, not before it). Today the ordered arms compare `to_string()` output, where `"2026-10-29T14:03:11Z"` sorts after `"2026-10-29"`: `-lt` and `-gte` already behave this way, and the change flips `--where-gt` (a same-day row is no longer after the date) and `--where-lte` (it is now on or before it). The rule is engine-wide, so it also reaches `inputs.toml` datetimes and `--newer-than`. It applies only where the field reaches the engine as a TOML datetime: JSON row reports filter through `query::json_rows`, where a date stays a plain string.

### Focus queue semantics

The queue is the live (`open` or `promoted`) rows carrying `focus`, ordered by `(focus, id)`; a row's **position** (1..N) is its index in that order, computed at read time and never stored. A new module `tomlctl/src/backlog/focus.rs` owns a pure planner `plan(queue, op) -> Result<Vec<FocusChange { id, before: Option<i64>, after: Option<i64> }>>` over `op`:

- `Add { ids, at: Option<usize> }` — rows not yet queued enter at position `at` (default: bottom), in the order given; the new rows take consecutive `focus` values starting at the `focus` of the row now at position `at` (max `focus` + 1 at the bottom; never `at` itself, since gaps make position and `focus` differ), and every queued row at position ≥ `at` shifts its `focus` by +len. `at` > N+1 clamps to N+1; `at` = 0 is refused. Re-adding a queued row is refused (use a move).
- `Remove { ids }` — drops `focus`; the gap stays (no renumbering). It also accepts a `resolved`/`dismissed` row still carrying `focus` — the only way to clear a stray rank an older binary, a merge or a hand edit left behind.
- `Move { id, up: bool }` — swaps `focus` with the neighbour; at either end it is a no-op change set.

When the queue holds a duplicate `focus` value, the planner first renumbers the whole queue 1..N in `(focus, id)` order and reports those rows as changed; this is the only renumber. The CLI verb `backlog focus` and the facade apply the plan in one `mutate_doc_conditional` call under the store lock, so a move's two rows land together or not at all. The facade additionally takes `expect: Vec<(id, Option<i64>)>` — the queue glimpse displayed — and writes nothing when the stored queue differs — an expected id is no longer live, its stored `focus` differs from the expected one, or a live row absent from `expect` carries `focus` (a row captured or triaged meanwhile without a rank is not a difference) — reporting each mismatching row in `skipped_stale` as a `{id, field: "focus", expected, found}` object (the `StaleOp::to_json` shape glimpse's `writer::Stale` deserialises; any other shape is silently dropped): `StalePolicy::Abort` semantics, not `write_guarded`'s per-row Skip. On success `applied` lists every id in `changed`, shifted and renumbered neighbours included, because glimpse's undo restores only `applied` ids. The CLI computes against the current store under the lock and needs no expectation.

### `backlog next`

A read verb in a new `tomlctl/src/backlog/next.rs`, printing a row report (bare array of full stored rows, each with an added `position`): first the `open` queued rows in queue order (`position` = queue position, counted over the whole live queue so it matches glimpse), then with `--fill` every unqueued `open` row oldest-first by `created` (bare date and datetime compare via `Datetime` `Ord`; equal values keep file order — a stable sort over file order), `position` absent. `--tier T` drops rows whose `tier` is set to something other than `T`; untiered rows pass, because tier is an opt-in routing mark and an untiered row falls back to the caller's own eligibility rule. A strict filter is the global `--where tier=T`, which already applies to any row report. `promoted` rows never appear (a flow has claimed them). The rows print as a JSON row report, so the global `--where` treats `created` as a plain string there and same-day matching does not apply to `next` output.

### Library facade

`tomlctl/src/lib.rs` adds, beside `backlog_triage`: `backlog_set_tier(root, ids, tier: Option<&str>, expect: Map)` (per-row CAS on the displayed `tier`, `None` = remove; validates through `schema::validate`; returns `{applied, skipped_stale}` like `ledger_classify`), `backlog_focus(root, op: BacklogFocus, expect: Vec<(String, Option<i64>)>)` (returns `{applied, skipped_stale, changed: [{id, before, after}]}` in the shapes "Focus queue semantics" fixes), and `backlog_focus_plan(queue, &BacklogFocus)` (the pure planner, exported so glimpse can predict a write's changed rows). `BacklogFocus` is a public enum re-exported like `BacklogTriage`; `FocusChange` is re-exported as `BacklogFocusChange` so glimpse can name the planner's result, and `TIERS` as `BACKLOG_TIERS` (like `BACKLOG_MANAGED_FIELDS`) so glimpse ranks and offers tiers from the one list. `ledgers::restorable(LedgerRef::Backlog)` adds `tier` and `focus` so undo can put both back.

### glimpse undo for queue moves

glimpse builds undo at dispatch time (`Writes::dispatch` pushes the `UndoEntry` before the result). For a queue write it calls `tomlctl::backlog_focus_plan` over the queue it displays — the same queue it sends as `expect`, so when the write lands its changes are exactly the predicted ones — and pushes one `RowUndo` per changed row (`set focus = before` or `unset focus`, `expect focus = after`). When the prediction is empty (a move at either end of the queue) glimpse dispatches nothing, and it marks the predicted changed ids as saving rather than the targets: a saving mark clears only on a stale outcome or when a read shows the row changed, so an untouched row would stay `↻`. A tier write uses the existing `classify_plan` grouping (identical `(fields, expect)` per request), with a clear represented as `{tier: null}` so it stays in the map `RowUndo::of` derives its restore `expect` from. A backlog resolve/dismiss now also lists `focus` among its touched fields so its undo restores the rank.

### glimpse surface

`BacklogRow` (`glimpse/src/ledger.rs`) deserialises every stored field; `ItemRow` gains only what sort, group and columns need (`tier`, `focus: Option<i64>`, `last_seen`, `seen_count: Option<i64>`, `origin`, `flow`); details read the rest from `raw`. In `glimpse/src/surface.rs` the Backlog surface adds sorts `LastSeen`, `SeenCount`, `Closed`, `Tier` (vocabulary rank), `Focus`, and groups `Tags` (the row's sorted tag set joined with `, `; an owned key, so `group_key` moves from `&str` to `Cow<'_, str>`), `Origin`, `Flow`, `Tier`. Newest (and LastSeen) break ties by file position instead of id: `visible()` sorts `(index, &ItemRow)` pairs from `self.rows`, so no position field enters `ItemRow` (which would make shifted rows compare unequal and clear `saving` marks). When any shown row is queued, `visible()` emits a `focus` header and the queued rows in queue order first, then the usual sort/group over the rest. `ItemsState` exposes `queue_positions() -> HashMap<String, usize>` for the rank column and details.

### Template: per-surface key reuse

`e` stays `Action::OpenClassify`; on the Backlog surface it opens a single-select `tier` form (`lite`, `deep`, `human`, `none`, plus the existing keep sentinel) over the marks, else the cursor row. `P`, `<`, `>` are new `Action`s, Backlog-only, no-ops elsewhere. `P` toggles: if every target is queued it removes them, otherwise it adds the unqueued ones at the bottom.

## Success Criteria

- forward: `grep -c 'FIELD_TIER\|FIELD_FOCUS' tomlctl/src/backlog/schema.rs` prints at least `2` (today: `0`).
- forward: `grep -c 'Next {\|Focus {' tomlctl/src/cli/types/backlog.rs` prints `2` (today: `0`).
- forward: `grep -c 'pub fn backlog_set_tier\|pub fn backlog_focus(' tomlctl/src/lib.rs` prints at least `2` (today: `0`).
- forward: `grep -c 'Sort::Focus\|Group::Tier' glimpse/src/surface.rs` prints at least `2` (today: `0`).
- forward: `grep -c 'backlog next' claude/skills/backlog-clear/SKILL.md` prints at least `1` (today: `0`).
- forward: after `cargo install --path tomlctl`, a fresh capture's `created` is a datetime: in a scratch repo `tomlctl backlog add --summary probe --kind other --area x --get id` then `tomlctl backlog get <id> --get created` prints a value matching `T..:..:..Z` — **predicted, unverified** (needs the installed binary and a scratch store).
- forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(backlog_queue)'` passes — the binary task 11 adds checks the tomlctl goals end to end: `--where created=<date>` matches a datetime row, `backlog next --tier lite --fill` orders queue-then-oldest and passes untiered rows, and the focus verbs keep the queue order (today: no such binary) — **predicted, unverified**.
- forward: `cargo test --manifest-path glimpse/Cargo.toml -- newest_breaks_same_day_ties_by_file_position backlog_details_render_every_field` reports `2 passed` — the glimpse goals: same-day rows order by file position, and details show every stored field (today: neither test exists, `0 passed`) — **predicted, unverified**.
- guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast` and `cargo test --manifest-path glimpse/Cargo.toml` pass (pass today: 2248 and 619 tests) — **predicted, unverified** (runs at checkpoints).

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml && cargo build --manifest-path glimpse/Cargo.toml
test: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast && cargo test --manifest-path glimpse/Cargo.toml --no-fail-fast
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets && cargo clippy --manifest-path glimpse/Cargo.toml --all-targets && cargo fmt --manifest-path tomlctl/Cargo.toml -- --check && cargo fmt --manifest-path glimpse/Cargo.toml -- --check
test.rerun: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -j 1 -- --exact {ids}
test.timeout: 900
success: test "$(grep -c 'FIELD_TIER\|FIELD_FOCUS' tomlctl/src/backlog/schema.rs)" -ge 2
success: test "$(grep -c 'Next {\|Focus {' tomlctl/src/cli/types/backlog.rs)" -eq 2
success: test "$(grep -c 'pub fn backlog_set_tier\|pub fn backlog_focus(' tomlctl/src/lib.rs)" -ge 2
success: test "$(grep -c 'Sort::Focus\|Group::Tier' glimpse/src/surface.rs)" -ge 2
success: grep -q 'backlog next' claude/skills/backlog-clear/SKILL.md
success: cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests
success: cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(backlog_queue)'
success: cargo test --manifest-path glimpse/Cargo.toml -- newest_breaks_same_day_ties_by_file_position backlog_details_render_every_field 2>&1 | grep -q 'test result: ok. 2 passed'
transient: rust-lld: failed to write output.*[Pp]ermission denied
```

The doc gates (`cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`) must pass at checkpoint C: they parse every fenced `tomlctl` line in `claude/`, so docs naming `backlog next`/`backlog focus` only pass once M1's verbs exist.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 10, 11, 12, 15, 16, 19, 20, 22, 23, 24, 25, 26, 27
- **Max parallel agents**: 6
- **Commit granularity**: per-task

## Tasks

### M1 — tomlctl

### 1. Move the datetime minting helper into time.rs [S]
- **Files**: `tomlctl/src/time.rs`, `tomlctl/src/inputs.rs`
- **Depends on**: —
- **Action**: Move `inputs.rs`'s private `fn now() -> Result<Datetime>` to `time.rs` as `pub(crate) fn now_toml_datetime() -> Result<toml::value::Datetime>` and call it from `inputs.rs`.
- **Detail**: Body unchanged: `jiff::Timestamp::from_second(now().as_second())`, then `to_string().parse::<Datetime>()` — whole seconds, `Z` offset, no `.0`. Add a unit test in `time.rs` pinning the clock with `set_now_for_test` and asserting the rendered string has the shape `YYYY-MM-DDTHH:MM:SSZ`.
- **Acceptance**:
  - forward: `grep -c 'fn now_toml_datetime' tomlctl/src/time.rs` prints `1` (today: `0`).
  - forward: `grep -c '^fn now()' tomlctl/src/inputs.rs` prints `0` (today: `1`).

### 2. Same-day matching for a bare-date RHS in the query engine [M]
- **Files**: `tomlctl/src/query.rs`, `tomlctl/src/convert.rs`
- **Depends on**: —
- **Action**: Route every datetime comparison through one helper that compares only the field's date component when the RHS is a bare date.
- **Detail**: Approach, "Same-day date matching". Sites in `query.rs`: the cached `ParsedRhs::Datetime` equality (`field_dt.to_string() == dt.to_string()`), `json_matches_toml`'s `(String, Datetime, Date|DateTime)` arm, the untyped `TomlValue::Datetime(dt) => dt.to_string() == rhs` arm (parse a 10-char RHS as a date there), and the ordered `ParsedRhs::Datetime` arm returning `field_dt.to_string().cmp(&dt.to_string())`; in `convert.rs`, `compare_typed`'s Datetime arm. A bare date is a `toml::value::Datetime` with `date: Some`, `time: None`, `offset: None`. Approach, "Same-day date matching" also names the operators that flip. Add inline tests: `--where created=2026-10-29` (untyped and `@date:`) matches a `2026-10-29T14:03:11Z` row; `--where-gt` excludes it and `--where-lte` includes it (these two flip from today's `to_string()` order, so the falsifier rests on them); `--where-lt` excludes it and `--where-gte` includes it (true before and after); a full-datetime RHS still matches exactly.
- **Acceptance**:
  - forward: `grep -c 'dt.to_string() == rhs' tomlctl/src/query.rs` prints `0` (today: `1`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- query::` passes with the new tests present — **predicted, unverified**.
  - falsifier: on today's tree `--where created=2026-10-29` misses a `2026-10-29T14:03:11Z` row and `--where-gt created=2026-10-29` keeps it, so the equality, `-gt` and `-lte` tests go red if the helper is reverted — **predicted, unverified**.

### 3. Backlog schema: tier and focus fields [M]
- **Files**: `tomlctl/src/backlog/schema.rs`
- **Depends on**: —
- **Backlog**: B-ec0c911b
- **Action**: Add `FIELD_TIER`, `FIELD_FOCUS`, `TIERS`, their `validate` rules and the `clear_for_transition` focus rule; rewrite the module header's evidence paragraph.
- **Detail**: Approach, "Fields and validation". New `BacklogError` variants `UnknownTier { tier }` and `InvalidFocus { found }` with messages naming the allowed values; a `focus` on `dismissed`/`resolved` reuses `ForeignTerminalField`. Read `focus` with `as_i64()` so a negative or float value is rejected, not read as absent. Declare `pub const TIERS` (task 10 re-exports it as `BACKLOG_TIERS`). Extend `vocabularies_are_pinned` to `TIERS`. Add tests: each rule rejects; `clear_for_transition(.., "resolved")` and `"dismissed"` remove `focus`; `"promoted"` and `"open"` keep it; `tier` survives all four. Header: replace "The store carries no evidence field of any kind…" with one accurate sentence — `evidence` holds refs written by `backlog add`/`edit`, while the evidence directory listing is computed by `show` at read time (B-ec0c911b).
- **Acceptance**:
  - forward: `grep -c 'carries no evidence field' tomlctl/src/backlog/schema.rs` prints `0` (today: `1`).
  - forward: `grep -c '^pub const TIERS' tomlctl/src/backlog/schema.rs` prints `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- backlog::schema::` passes with the new rule tests present; reverting the `clear_for_transition` focus rule turns the resolved/dismissed tests red — **predicted, unverified**.

### 4. Captures stamp created and last_seen as datetimes [M]
- **Files**: `tomlctl/src/backlog/add.rs`, `tomlctl/src/backlog/add_many.rs`
- **Depends on**: 1, 3
- **Action**: Add `stamp: toml::value::Datetime` to `AddRequest`, write it in `build_row` (`created`, `last_seen`) and `bump_row` (`last_seen`); accept `tier` in payloads and refuse `focus`.
- **Detail**: Approach, "Datetime captures". Resolve `stamp` once per call beside `today` (`crate::time::now_toml_datetime()`) in `build_request` and `add_many::parse_rows`, threading it through `payload_request`. `stamp_root` keeps `today`. In `unknown_payload_key` accept `FIELD_TIER` and `FIELD_FOCUS`; `request_from_payload` drops `focus` the way it drops the minted fields, so a replayed `show`/`get` of a queued row still adds without planting a rank. The existing test asserting `row[FIELD_CREATED].as_datetime().is_some()` keeps passing; add one asserting the value has a time and a `Z` offset, one that an invalid `tier` payload is refused by `schema::validate`, and one that a payload's `focus` does not reach the row.
- **Acceptance**:
  - forward: `grep -c 'Datetime(req.stamp)' tomlctl/src/backlog/add.rs` prints `3` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- backlog::add` passes with the new tests present; writing `req.today` back into `build_row` turns the `Z`-offset test red — **predicted, unverified**.

### 5. backlog edit sets and clears tier [M]
- **Files**: `tomlctl/src/backlog/edit.rs`, `tomlctl/src/store_guard.rs`
- **Depends on**: 3
- **Action**: Add `tier` to `EDITABLE_FIELDS` and `Edit` (`tier: Option<Option<String>>`, `Some(None)` = remove); an `--ndjson` row's `"tier": null` removes it; `refusal_for("focus")` names `backlog focus`. In `store_guard.rs`, the `OwnedStore::Backlog` refusal names `tier` under `backlog edit` and `backlog focus` for `focus`.
- **Detail**: Validation stays with `schema::validate` via the existing clone-and-validate path in `apply_edits`. Update the module header's editable-field list. The clap flags (`--tier <lite|deep|human>`, `--clear-tier`) and the matching `tier` parameter on `edit::dispatch` land in task 8; this task adds only the `Edit` field and the `--ndjson` row key. Unit tests: set, replace, clear, bad value refused, `focus` refused with the owner named.
- **Acceptance**:
  - forward: `grep -c 'FIELD_TIER' tomlctl/src/backlog/edit.rs` prints at least `1` (today: `0`).
  - forward: `grep -c 'backlog focus' tomlctl/src/store_guard.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- backlog::edit::` passes with the new tests present — **predicted, unverified**.

### 6. Focus queue planner and writer [M]
- **Files**: `tomlctl/src/backlog/focus.rs` (new), `tomlctl/src/backlog/mod.rs`
- **Depends on**: 3
- **Action**: Create `focus.rs` with the pure planner, the locked apply used by both CLI and facade, and the CLI entry `run_focus`; register `mod focus` (as `pub(crate)`, so `lib.rs` can reach it).
- **Detail**: Approach, "Focus queue semantics". Public-in-crate surface: `pub enum FocusOp { Add { ids: Vec<String>, at: Option<usize> }, Remove { ids: Vec<String> }, Move { id: String, up: bool } }` (task 10 re-exports it as `tomlctl::BacklogFocus`), `pub struct FocusChange { pub id, pub before: Option<i64>, pub after: Option<i64> }`, `pub fn plan(queue: &[(String, Option<i64>)], op: &FocusOp) -> Result<Vec<FocusChange>>` (queue = live rows' `(id, focus)`, any order), `pub(crate) fn apply(root, op, expect: Option<&[(String, Option<i64>)]>, write_opts, dry_run) -> Result<JsonValue>` — one `mutate_doc_conditional`, `schema::validate` on every changed row, `last_updated` stamped with `today_toml_date()`; with `expect` set and the stored queue different (as Approach, "Focus queue semantics" defines it), write nothing and return one `{id, field: "focus", expected, found}` object per mismatching row under `skipped_stale`. Errors (`kind=validation`): unknown id, a non-live row for `Add`/`Move` (`Remove` accepts a `resolved`/`dismissed` row still carrying `focus`), `Add` of a queued row, `Remove`/`Move` of an unqueued row, `at` = 0. Output: `{"ok":true, "applied":[every changed id], "skipped_stale":[…], "changed":[{id,before,after}], "queue":[ids in order], "dry_run"?}`. Unit tests cover: add at bottom; insert at k shifts later rows; add into a queue with a gap (`[1, 2, 5]` + bottom gives `6`, not `4`); `at` beyond N+1 clamps; remove leaves a gap; remove of a terminal row's stray `focus`; move up/down swaps; end-of-queue move is empty; duplicate values renumber first; stale `expect` writes nothing; a concurrent unranked capture is not stale.
- **Acceptance**:
  - forward: `test -f tomlctl/src/backlog/focus.rs && grep -c 'mod focus' tomlctl/src/backlog/mod.rs` prints `1` (today: no file, `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- backlog::focus::` passes with the planner tests present; writing `at` as the new row's `focus` turns the gapped-queue test red — **predicted, unverified**.

### 7. backlog next read verb [M]
- **Files**: `tomlctl/src/backlog/next.rs` (new), `tomlctl/src/backlog/mod.rs`
- **Depends on**: 3, 6
- **Action**: Create `next.rs` with `pub(crate) fn dispatch_next(tier: Option<&str>, fill: bool, integrity: ReadIntegrityArgs) -> Result<()>` and a pure `select(doc, tier, fill) -> Vec<JsonValue>`; register `mod next`.
- **Detail**: Approach, "`backlog next`". Read the store through `schema::read_store` (missing store → empty array, as `list` does). Queue positions come from `focus::plan`'s ordering helper (reuse, do not reimplement `(focus, id)` ordering — expose a `pub(crate) fn ordered(queue) -> Vec<(String, i64)>` from `focus.rs` if task 6 did not). Print with `crate::output::print_report(JsonValue::Array(rows), Rows::Top)` (the row-report path `backlog check` uses; `backlog list`'s `print_query_list` needs a `QueryArgs`-built `Query`, which `Next` does not carry), so the global output options (`--get`, `--template`, `--where`, `--limit`) apply and a one-row result is still an array. Row-report `--where` runs `query::json_rows::filter`, where a date is a plain string, so same-day matching does not apply to `next` output; task 21 documents that. Depends on 6 for `mod.rs` and the ordering helper. Unit tests: ranked order; `position` over the whole live queue (a promoted queued row still counts); `--fill` oldest-first with a bare-date row before a same-day datetime row and file order on equal values; `--tier lite` drops `deep`/`human` rows and keeps untiered ones; promoted rows absent.
- **Acceptance**:
  - forward: `test -f tomlctl/src/backlog/next.rs && grep -c 'mod next' tomlctl/src/backlog/mod.rs` prints `1` (today: no file, `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- backlog::next::` passes with the selection tests present; dropping untiered rows under `--tier` turns the pass-through test red — **predicted, unverified**.

### 8. CLI wiring for next, focus and edit --tier [M]
- **Files**: `tomlctl/src/cli/types/backlog.rs`, `tomlctl/src/backlog/dispatch.rs`, `tomlctl/src/backlog/edit.rs` — `edit::dispatch` gains the `tier` parameter
- **Depends on**: 5, 6, 7
- **Action**: Add `BacklogOp::Next` and `BacklogOp::Focus`, `--tier`/`--clear-tier` on `Edit`, a `BacklogTier` `ValueEnum` (`lite`, `deep`, `human`), and their dispatch arms; add a `tier: Option<Option<String>>` parameter to `edit::dispatch`, threaded into the `Edit` it builds and its `changes_nothing` check.
- **Detail**: Type `--tier` as `#[arg(long, value_enum, ignore_case = true)] tier: Option<BacklogTier>` (pattern: `DupTier` in `tomlctl/src/cli/types/items.rs`) so `capabilities` enumerates `lite/deep/human` ahead of the global `tier → A/B/C` fallback; add no `SCOPED_ENUM_VALUES` entry. `Next { tier, fill: bool, integrity: ReadIntegrityArgs }`. `Focus { ids: ItemIds, at: Option<usize> (--at K), remove: bool, up: bool, down: bool, dry_run, integrity: WriteIntegrityArgs }` with `remove`/`up`/`down`/`at` mutually exclusive and `up`/`down` requiring exactly one id (validated in dispatch as a tagged `kind=validation` error, following the `--json` conflict pattern in `add.rs`). `Edit`: `--tier` conflicts with `--clear-tier` and `--ndjson`; its doc comment and the `--ndjson` row-shape doc, which list the editable fields, gain `tier`. Doc comments are the `--help` text; keep each ≤ 4 lines. Add a unit test asserting `BacklogTier::value_variants()` names equal `crate::backlog::schema::TIERS`.
- **Acceptance**:
  - forward: `grep -c 'Next {\|Focus {' tomlctl/src/cli/types/backlog.rs` prints `2` (today: `0`).

### 9. Capabilities, feature list and README [M]
- **Files**: `tomlctl/src/cli/types/mod.rs`, `tomlctl/tests/capabilities.rs`, `tomlctl/README.md`
- **Depends on**: 4, 8
- **Action**: Add features `backlog_next`, `backlog_focus`, `backlog_tier`, `backlog_datetime_stamps` to `FEATURES` and the mirrored expected array; list `backlog next` in `read_only_subcommands_hide_write_integrity_flags_in_help` and `backlog focus` in `write_subcommands_expose_all_integrity_flags_in_help`; add the four rows to both README feature tables and the verbs to its backlog section.
- **Detail**: The README has a feature-name list (near `"backlog_get", "backlog_edit"`) and a feature table (`| \`backlog_edit\` | …`); update both. Keep each table row one line. Also amend the existing `backlog_edit` feature row and the `backlog edit --ndjson` synopsis row, which list the editable fields and say "only", to include `tier`. Depends on 4 because `backlog_datetime_stamps` advertises its behaviour.
- **Acceptance**:
  - forward: `grep -c 'backlog_next\|backlog_focus\|backlog_tier\|backlog_datetime_stamps' tomlctl/src/cli/types/mod.rs` prints `4` (today: `0`).

### 10. Library facade: tier, focus and restore [M]
- **Files**: `tomlctl/src/lib.rs`, `tomlctl/src/ledgers.rs`, `tomlctl/tests/library_facade.rs`
- **Depends on**: 3, 6
- **Action**: Add `backlog_set_tier`, `backlog_focus`, `backlog_focus_plan` and the `BacklogFocus` re-export to `lib.rs`; implement the tier write in `ledgers.rs` over `write_guarded`; add `tier` and `focus` to `restorable(LedgerRef::Backlog)`.
- **Detail**: Approach, "Library facade". Each facade fn calls `io::silence_advisories()` and `ensure_process_root` like `backlog_triage`, and refuses a missing store (`no_create: true`). `backlog_set_tier(root, ids, tier: Option<&str>, expect: Map)` → `RowEdit { set: {tier} or unset: [tier], expect }` per id, validated with `backlog::schema::validate`. `backlog_focus` delegates to `focus::apply` with `Some(expect)`. Re-export `focus::FocusChange` as `BacklogFocusChange` and `schema::TIERS` as `BACKLOG_TIERS`. Tests in `tomlctl/tests/library_facade.rs`: tier set/clear and stale skip; focus add/move with matching expect, `applied` naming the shifted neighbours; mismatched expect writes nothing and reports `StaleOp`-shaped `skipped_stale` entries; restore of `focus` and `tier` accepted.
- **Acceptance**:
  - forward: `grep -c 'pub fn backlog_set_tier\|pub fn backlog_focus(' tomlctl/src/lib.rs` prints `2` (today: `0`).
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(library_facade)'` passes with the new tests present — **predicted, unverified**.

### 11. Integration tests for the queue verbs and datetimes [M]
- **Files**: `tomlctl/tests/backlog_queue.rs` (new), `tomlctl/tests/output_options.rs` — `cases()` entries for the two new leaves
- **Depends on**: 2, 4, 8
- **Backlog**: B-fa9da685
- **Action**: Add binary-level tests driving `tomlctl backlog focus`, `next`, `edit --tier`, `add`, and `list --where created=<date>` over a temp repo; add a `read(&["backlog", "next"], Top, "id")` case and a `write(&["backlog", "focus", "{B1}"], …)` case to `cases()` in `tomlctl/tests/output_options.rs`, since `every_leaf_command_has_a_case` fails on any leaf command without one.
- **Detail**: Follow `tomlctl/tests/backlog_edit.rs` for the temp-repo and binary helpers in `tomlctl/tests/common`. Cases: `focus A,B` then `focus C --at 1` yields queue `C,A,B`; `--up`/`--down`; `--remove` then `next` positions; `next --tier lite --fill` ordering and the untiered pass-through; `edit --tier deep`/`--clear-tier`; a fresh `add` row's `created` ends `Z`; `list --where created=<today>` finds it; `resolve` drops `focus`, `promote` keeps it; `capabilities` lists `lite`,`deep`,`human` for `backlog next --tier`.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(backlog_queue)'` passes — **predicted, unverified** (file does not exist yet).
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(output_options)'` passes, including `every_leaf_command_has_a_case` — **predicted, unverified** (red from task 8 until the two cases land).

### 12. Bump tomlctl to 0.17.0 [S]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `tomlctl/README.md` — the `capabilities` sample's `"version"`, `tomlctl/tests/capabilities.rs` — `capabilities_version_matches_cargo_toml`'s pinned version
- **Depends on**: 9
- **Action**: Set `version = "0.17.0"` in `tomlctl/Cargo.toml`, refresh both lockfiles (`cargo tree --manifest-path tomlctl/Cargo.toml >/dev/null` and `cargo tree --manifest-path glimpse/Cargo.toml >/dev/null`), and move the two version pins: the `"version": "0.16.0"` line in the README's `capabilities` sample (checked by `readme_sample_version_matches_cargo_toml`) and the `"0.16.0"` assertion and message in `capabilities_version_matches_cargo_toml`.
- **Detail**: The pre-commit hook runs `cargo clippy --manifest-path glimpse/Cargo.toml --locked` on tomlctl changes, so `glimpse/Cargo.lock` must be staged with the bump.
- **Acceptance**:
  - forward: `grep -c '^version = "0.17.0"' tomlctl/Cargo.toml` prints `1` (today: `0`).
  - forward: `grep -A1 '^name = "tomlctl"$' glimpse/Cargo.lock | grep -c '^version = "0.17.0"$'` prints `1` (today: `0`).
  - forward: `grep -c '0\.16\.0' tomlctl/tests/capabilities.rs tomlctl/README.md` prints `0` for both files (today: at least `1` each).

### M2 — glimpse

### 13. Backlog rows carry every stored field [M]
- **Files**: `glimpse/src/ledger.rs`, `glimpse/tests/fixtures/backlog.toml`
- **Depends on**: —
- **Action**: Extend `BacklogRow` with `last_seen`, `seen_count`, `origin`, `flow`, `base_sha`, `related`, `duplicate_of`, `supersedes`, `resolved_flow`, `resolved_tasks`, `resolved_commits`, `tier`, `focus`. `ItemRow` gains six fields — tier, focus (`Option<i64>`), last_seen, seen_count (`Option<i64>`), origin, flow — filled in `ItemRow::backlog`.
- **Detail**: Approach, "glimpse surface". `lenient` drops a type-mismatched field, so a non-integer `focus` reads as absent. Do not add a file-position field to `ItemRow`. Add new fixture rows (leave the existing four unchanged, so `writer.rs`'s `a_backlog_dismissal_and_its_undo_leave_the_row_as_it_was`, which dismisses `B-0a1b2c3d` through the facade, is unaffected) covering: a datetime `created`, two rows sharing a `created` date, `tier` values, a three-row queue with a gap, a promoted queued row. Update `backlog_rows_carry_kind_and_area` and add a test for the new fields. The `ItemRow.created` doc comment ("as `YYYY-MM-DD`") becomes "a date or an RFC 3339 `Z` datetime".
- **Acceptance**:
  - forward: `grep -c 'focus' glimpse/src/ledger.rs` prints at least `2` (today: `0`).
  - forward: `cargo test --manifest-path glimpse/Cargo.toml -- ledger::` passes with the new-fields test present — **predicted, unverified**.

### 14. Backlog sorts, groups and the pinned focus block [L]
- **Files**: `glimpse/src/surface.rs`, `glimpse/src/cli.rs` — the backlog group list asserted in `the_arrangement_must_be_offered_by_the_surface`
- **Depends on**: 10, 13
- **Action**: Add `Sort::{LastSeen, SeenCount, Closed, Tier, Focus}` and `Group::{Tags, Origin, Flow, Tier}` to the enums, their `label`/`key`, `Surface::Backlog`'s `sorts()`/`groups()`, `compare` and `group_key`; make Newest/LastSeen tie-break on file position; pin queued rows above everything; add `ItemsState::queue_positions()`.
- **Detail**: Approach, "glimpse surface". Keys are stable `state.toml` names: `last-seen`, `seen-count`, `closed`, `tier`, `focus`, `tags`, `origin`, `flow` — append to `saved_keys_stay_fixed` and `group_and_sort_cycle_through_the_surface_options` (both in this file's tests), never rename existing ones. Tier ranks in `tomlctl::BACKLOG_TIERS` order (`lite, deep, human`; re-exported by task 10), unknown after, blank last. Focus sort orders queued rows by `(focus, id)`, unqueued last. File-position tie-break: sort `(index, &ItemRow)` pairs enumerated from `self.rows`; Newest = later index first on equal `created`. Pinned block: when any shown row is live and queued, emit `VisibleRow::Header { label: "focus", count }` then those rows by `(focus, id)`, then the normal output over the remaining rows (their headers only if grouping is on). `Group::Tags` needs an owned key, so `group_key` returns `Cow<'_, str>`. `glimpse/src/cli.rs`'s `the_arrangement_must_be_offered_by_the_surface` asserts the exact backlog group list ("…offers none, kind, area, status"); extend it with the four new groups. Tests: tie-break (named `newest_breaks_same_day_ties_by_file_position`, which `## Success Criteria` runs), each new sort/group, the pin with grouping on and off, positions skip a gap. Tagged L to force `implement-deep`: the pinned block reorders the `visible()` output every Backlog view relies on.
- **Acceptance**:
  - forward: `grep -c 'Sort::Focus\|Group::Tier' glimpse/src/surface.rs` prints at least `2` (today: `0`).
  - forward: `cargo test --manifest-path glimpse/Cargo.toml -- surface:: cli::` passes with the new tests present; sorting Newest ties by id again turns `newest_breaks_same_day_ties_by_file_position` red — **predicted, unverified**.

### 15. Details show every backlog field [M]
- **Files**: `glimpse/src/view/details.rs`
- **Depends on**: 14
- **Action**: In `item_content`, for a backlog row render every stored field with a `fact` line — tier, queue position (`#k of N`, from `ItemsState::queue_positions()`), created and last_seen (dates shown as stored), seen_count, origin, flow, base_sha, relations, resolution links — then each remaining non-empty `raw` key not already shown.
- **Detail**: Follow the existing `fact(label, value, style)` closure and the dates line. "Remaining" means keys of `row.raw` not in the explicit list and not already rendered by `item_content` — `id`, `summary`, `context`, `status`, `kind`, `area`, `tags`, `evidence`, the companions `resolution`/`dismiss_reason`/`promoted_to`/`reopen_rationale`, and the `promoted`/`dismissed`/`resolved` dates; `created` stays on the existing dates line rather than becoming a second fact. Render scalars as text and arrays comma-joined. Add the first backlog details test, named `backlog_details_render_every_field` (`## Success Criteria` runs it): a fixture row with every field renders each label once, and an unknown extra key (`custom = "x"`) is shown.
- **Acceptance**:
  - forward: `grep -c 'seen_count' glimpse/src/view/details.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path glimpse/Cargo.toml -- backlog_details_render_every_field` reports `1 passed` — **predicted, unverified** (the test does not exist yet).

### 16. Tier and rank columns; titles count toward column width [M]
- **Files**: `glimpse/src/config.rs`, `glimpse/src/view/items.rs`, `glimpse/src/surface.rs` — `column_sort` arms for the new columns
- **Depends on**: 14
- **Backlog**: B-00b994ba
- **Action**: Add `ItemColumn::Tier` and `ItemColumn::Rank` (rank cell `#k` from `queue_positions()`), included in `ALL` and the default list; make `Columns::measure` take each visible column's title width into account.
- **Detail**: A zero-width column is hidden, so both new columns vanish on non-backlog surfaces and when no row has a tier/rank. Saved `item_columns` replace the default list, and glimpse saves the list on every quit, so every existing install opts in with `C` — note it in task 20's README change, not here. `Surface::column_sort` in `glimpse/src/surface.rs` matches `ItemColumn` exhaustively (no `_` arm), so give it `ItemColumn::Tier => Sort::Tier` and `ItemColumn::Rank => Sort::Focus` arms (both sorts land in task 14; its `sorts().contains` filter still yields `None` off Backlog). B-00b994ba: measuring titles widens some columns by a cell; update the row-content offset assertions in `view::items` tests accordingly, and add a test named `column_width_counts_the_title` where a column's title is wider than every cell.
- **Acceptance**:
  - forward: `grep -c 'Rank' glimpse/src/config.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path glimpse/Cargo.toml -- column_width_counts_the_title` reports `1 passed`; reverting `Columns::measure` to cell widths only turns it red — **predicted, unverified**.

### 17. Write requests for tier and the queue [M]
- **Files**: `glimpse/src/actions.rs`, `glimpse/src/writer.rs`
- **Depends on**: 10, 13
- **Action**: Add `tier_form`, `tier_plan` (the `classify_plan` grouping over `tier`), `focus_plan` (calls `tomlctl::backlog_focus_plan` and builds one `RowUndo` per change), `WriteRequest::{BacklogTier, BacklogFocus}` and their `perform` arms; add `focus` to a backlog transition's undo touched fields. The `Purpose::Tier(Selection)` variant lands in task 18 with its `App::submitted` arm, since that match is exhaustive.
- **Detail**: Approach, "glimpse undo for queue moves". `tier_plan`: per row, `expect = {tier: raw tier or null}`, `fields = {tier: value}`, or `{tier: null}` for `none` — the clear must stay in the map handed to `RowUndo::of`, whose `expect` becomes the restore precondition, and `ledgers::restore` refuses an empty one; rows already at the value are left out. Tier options come from `tomlctl::BACKLOG_TIERS`. `focus_plan(queue: &[(String, Option<i64>)], op) -> Plan`: one `WriteRequest::BacklogFocus { request, op, expect: queue }` tracking the predicted changed ids, or an empty `Plan` when the prediction is empty. In `transition_plan`'s backlog branch push `"focus"` after `BACKLOG_MANAGED_FIELDS`. Tests beside the existing `classify_plan` tests in `glimpse/src/actions.rs`, plus a facade round-trip in `glimpse/src/writer.rs`'s sandbox: dismiss a queued fixture row, undo, and assert its `focus` is back.
- **Acceptance**:
  - forward: `grep -c 'BacklogFocus' glimpse/src/writer.rs` prints at least `2` (today: `0`).
  - forward: `cargo test --manifest-path glimpse/Cargo.toml -- actions:: writer::` passes with the new tests present; dropping `"focus"` from the transition's touched fields turns the queued-row dismissal round-trip red — **predicted, unverified**.

### 18. Keys and app actions: e tier, P, <, > [L]
- **Files**: `glimpse/src/keys.rs`, `glimpse/src/app.rs`, `glimpse/src/actions.rs` — the `Purpose::Tier(Selection)` variant, `glimpse/src/view/footer.rs` — Backlog hints
- **Depends on**: 14, 17
- **Action**: Bind `P` → `Action::ToggleQueue`, `<` → `Action::QueueUp`, `>` → `Action::QueueDown`; add the `Purpose::Tier(Selection)` variant in `glimpse/src/actions.rs` together with its `App::submitted` arm (that match is exhaustive, so both land in one task); on the Backlog surface `Action::OpenClassify` opens the tier form with `Purpose::Tier(selection)`; wire submission and the three queue actions to the task-17 plans through the existing dispatch path; in `glimpse/src/view/footer.rs` `surface_hints`, show `e tier` and `P queue` on Backlog.
- **Detail**: Approach, "Template: per-surface key reuse". Queue actions are no-ops off the Backlog surface and on read-only rows. `ToggleQueue` targets `ItemsState::targets()` (marks, else cursor); `QueueUp`/`QueueDown` target the cursor row only, and keep the cursor on that row after the write lands. Dispatch nothing when `tomlctl::backlog_focus_plan` predicts an empty change set (a move at either end of the queue), and mark the predicted changed ids as saving rather than the targets: glimpse clears a saving mark only when a read shows the row changed. The queue passed to `focus_plan` is every live row's `(id, focus)` in the current `ItemsState` rows. Tests: `keys::map` for `P`, `<`, `>` (and that `b` stays unbound); `e` on Backlog opens the tier form; `P` on a mixed selection adds only the unqueued rows; `<` on the top row dispatches nothing. Tagged L: four files whose edits land together.
- **Acceptance**:
  - forward: `grep -c "KeyCode::Char('P') =>\|KeyCode::Char('<') =>\|KeyCode::Char('>') =>" glimpse/src/keys.rs` prints `3` (today: `0`).
  - forward: `cargo test --manifest-path glimpse/Cargo.toml -- keys:: app::` passes with the new tests present — **predicted, unverified**.

### 19. Fold the legend's key rows and add the queue keys [S]
- **Files**: `glimpse/src/view/legend.rs`
- **Depends on**: —
- **Backlog**: B-be2797ef
- **Action**: Fold related `KEYS` rows so the table drops to at most 32 entries including the new one; add `P  < >` ("backlog: queue / move in the queue") and amend the `m  e` row to "action menu / classify or tier".
- **Detail**: Folds: `J K`, `S-PgUp S-PgDn` and `S-Home S-End` into one details-panel row; `o`, `|`, `[ ]` and `- =` into two layout rows; `z` and `Z` into one row; `i` and `F` into one row; `H` and `d` into one row. Add `"P"`, `"<"`, `">"` to `every_bound_letter_key_has_a_legend_row`, and add a test asserting `KEYS.len() <= 32`.
- **Acceptance**:
  - forward: `awk '/^const KEYS/,/^];/' glimpse/src/view/legend.rs | grep -c '^    ('` prints at most `32` (today: `38`).

### 20. glimpse README and CLAUDE.md [S]
- **Files**: `glimpse/README.md`, `glimpse/CLAUDE.md`
- **Depends on**: 18
- **Backlog**: B-fa9da685
- **Action**: Document the new keys, sorts, groups, the pinned focus block, the tier/rank columns (and that a saved `item_columns` — which every existing install has, since glimpse saves it on every quit — hides them until added with `C`), and amend the write model: glimpse writes backlog `tier` and `focus` through facade verbs besides triage status.
- **Detail**: README "Writing" says glimpse writes "every backlog field but its triage status" never; CLAUDE.md "Control fields only" and the write-model paragraph say the same, as do the README's opening "glimpse writes two things and nothing else" **Control fields** bullet and its `item_columns` default row (task 16 adds Tier and Rank to the default). Keep README under 600 lines (it is 592): replace text rather than add where possible.
- **Acceptance**:
  - forward: `grep -c 'tier' glimpse/CLAUDE.md` prints at least `1` (today: `0`).

### M3 — skills and docs

### 21. tomlctl skill: queue reference, edit tier, skill index [M]
- **Files**: `claude/skills/tomlctl/references/backlog-queue.md` (new), `claude/skills/tomlctl/references/backlog-edit.md`, `claude/skills/tomlctl/SKILL.md`
- **Depends on**: 9
- **Action**: Write `backlog-queue.md` (a `## Contents` list, `` ## `backlog next` `` and `` ## `backlog focus` `` headings each with a `| Flag |` table, queue semantics, output shapes, the `tier` field and its vocabulary, the `--where tier=T` strict form, and that same-day `--where created=<date>` matching does not apply to `next` output because it is a JSON row report); add `--tier`/`--clear-tier` and the `tier` row key to `backlog-edit.md`; add the reference to SKILL.md's References list, two Quick Reference rows and the four features to its feature table, and add `tier` to the existing Quick Reference row "Edit a backlog row's context, tags, evidence, origin, flow or base_sha".
- **Detail**: `backlog.md` is task 26's. Every fenced `tomlctl` line must parse (`command_lint`); every `| Flag |` table must sit under a `` ## `verb` `` heading (`flag_table_lint`). Run `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`; it parses against the in-tree CLI, so tasks 8 and 9 must have landed.
- **Acceptance**:
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes with the new file present (passes today: 46/46) — **predicted, unverified**.
  - forward: `grep -c 'backlog-queue.md' claude/skills/tomlctl/SKILL.md` prints at least `1` (today: `0`).

### 22. Query reference: same-day date matching [S]
- **Files**: `claude/skills/tomlctl/references/query.md`
- **Depends on**: 2
- **Action**: Under the filters section, state that a bare-date RHS matches a datetime field by its date (equality and ordering, so `--where-gt D` excludes and `--where-lte D` includes a row stamped on day `D`), and that it applies only to the list verbs' TOML engine, not to JSON row reports, with one example.
- **Detail**: Keep it to three sentences plus one fenced example that `command_lint` parses. Approach, "Same-day date matching".
- **Acceptance**:
  - forward: `grep -ci 'same day\|same-day' claude/skills/tomlctl/references/query.md` prints at least `1` (today: `0`).

### 23. backlog-clear picks from backlog next [M]
- **Files**: `claude/skills/backlog-clear/SKILL.md`
- **Depends on**: 12
- **Backlog**: B-fa9da685
- **Action**: Step 0 runs `tomlctl backlog next --tier lite --fill` (and gates on 0.17); Step 1 takes every `tier = lite` row regardless of kind, skips `deep`/`human` (already filtered), and applies the `debt|annoyance` default only to untiered rows; Step 2's created-versus-base-commit comparison reads the date part of a datetime `created`.
- **Detail**: Keep the version comment honest: `# backlog next landed in 0.17`. Do not touch `claude/commands/backlog-clear.md`.
- **Acceptance**:
  - forward: `grep -c 'backlog next --tier lite --fill' claude/skills/backlog-clear/SKILL.md` prints `1` (today: `0`).

### 24. backlog-capture and /backlog: tier, queue, datetimes [M]
- **Files**: `claude/skills/backlog-capture/SKILL.md`, `claude/commands/backlog.md`
- **Depends on**: 12
- **Backlog**: B-fa9da685
- **Action**: In backlog-capture's field vocabulary add `tier` (who may pick it up; set by the user or with `backlog edit --tier`) and `focus` (queue rank, owned by `backlog focus` and glimpse), say `created`/`last_seen` are UTC datetimes on new rows, and amend its "Who writes" paragraph ("triage rows in glimpse (dismiss, reopen, resolve)") to include setting a tier and queueing; in `/backlog` Step 2 add "set a tier" and "queue it" as dispositions with their commands, and raise the Step 0 version floor to 0.17.
- **Detail**: Commands go in fenced blocks that `command_lint` parses. backlog-capture is 405 lines; stay under 500 (`skill_bodies_under_line_ceiling` caps a SKILL.md at 500). `/backlog` lists its dispositions in three places — the Step 2 offer list, the Empty-answer rule's `question` options and the Step 0 drain's **Answers** handling — so state beside the new dispositions that they are interactive-only, rather than adding them to the other two.
- **Acceptance**:
  - forward: `grep -c 'backlog focus' claude/commands/backlog.md` prints at least `1` (today: `0`).

### 25. Glimpse backlog writes in the root CLAUDE.md [S]
- **Files**: `CLAUDE.md`
- **Depends on**: 18
- **Action**: In the root `CLAUDE.md` **Backlog capture** section, change "The human triages in glimpse (dismiss, reopen, resolve)" to include setting a tier and queueing.
- **Detail**: Touch only that statement; the root CLAUDE.md's stale `.github` mirror text stays out of scope. `claude/skills/flow-contract-ledger-schema/SKILL.md`'s glimpse writer list covers the review and optimise ledgers, not the backlog, so it needs no change.
- **Acceptance**:
  - forward: `grep -c 'triages in glimpse (dismiss, reopen, resolve)' CLAUDE.md` prints `0` (today: `1`).

### 26. Backlog store-shape row and recipe field list [S]
- **Files**: `claude/skills/tomlctl/references/backlog.md`, `claude/skills/tomlctl/references/backlog-recipes.md`
- **Depends on**: 21
- **Action**: Add one `tier`, `focus` row to `backlog.md`'s `## Store shape` table pointing at `backlog-queue.md`, folding one line elsewhere in the file so it stays at or under 600 lines; add `tier` to `backlog-recipes.md`'s list of `backlog edit`'s editable fields.
- **Detail**: `backlog.md` is at the 600-line ceiling (`skill_references_under_line_ceiling` fails above 600), so trade a line rather than add one. Depends on 21, which creates the `backlog-queue.md` link target.
- **Acceptance**:
  - forward: `grep -c 'backlog-queue.md' claude/skills/tomlctl/references/backlog.md` prints at least `1` (today: `0`).
  - guard: `test "$(wc -l < claude/skills/tomlctl/references/backlog.md)" -le 600` (holds today: `600`).

### 27. Open the tier form's select straight away [S]
- **Files**: `glimpse/src/actions.rs`, `glimpse/src/app.rs`
- **Depends on**: 17, 18
- **Action**: In `tier_form` (glimpse/src/actions.rs), build its single `Field::select("tier", options)` with `.opened()`, as the action menu's lone select does, so `e` on Backlog shows the tier list at once instead of needing an extra Enter. Add or extend a test asserting the tier form's select starts open.
- **Acceptance**:
  - forward: `grep -A12 'fn tier_form' glimpse/src/actions.rs | grep -c 'opened()'` prints `1` (today: `0`).
  - forward: `cargo test --manifest-path glimpse/Cargo.toml --bin glimpse -- actions::` passes with the open-select assertion present.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 10, 11, 12 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12. Tomlctl schema, verbs, facade and version bump (M1)

— CHECKPOINT B after tasks 15, 16, 19, 20, 27 — dependency closure: 3, 6, 10, 13, 14, 15, 16, 17, 18, 19, 20, 27. Glimpse surface, writes and docs (M2)

— CHECKPOINT C after tasks 22, 23, 24, 25, 26 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13, 14, 17, 18, 21, 22, 23, 24, 25, 26. Skills and command docs (M3)

## Risks

- **Undo of a queue move uses `restore`, whose per-row Skip can half-apply** if another writer moved one of the two rows meanwhile — rare (one user, one queue); the next read shows the true order and a second `P`/`<`/`>` fixes it. Not worth an Abort mode on `restore`.
- **Pre-existing date-only rows and new datetime rows mix** — ordering is chronological by construction (bare `D` sorts before `D T…Z`); same-day matching keeps `--where created=<date>` working on both; `compact.rs read_date` already reads either shape.
- **`--tier` keeps untiered rows** — a reader expecting a strict filter gets more rows; documented in `backlog-queue.md` with the `--where tier=T` strict form, and every row carries `tier` so callers can see it.
- **Saved glimpse column lists hide the new columns** on every existing install, since `State::capture` saves `item_columns` on every quit, not only after a customisation — documented in the README; the details panel shows tier and position regardless.
- **`state.toml` key drift** — new sort/group keys are appended and pinned in `saved_keys_stay_fixed`; existing keys untouched, so saved state still loads.
- **glimpse links its own tomlctl copy** — After Merge reinstalls both binaries; glimpse path-depends on tomlctl, so every checkpoint builds it against the in-tree tomlctl; the 0.17.0 bump and its `glimpse/Cargo.lock` refresh belong to checkpoint A.
- **Version skew leaves a stray `focus` on a terminal row** — a 0.16 binary, or a glimpse pane still linking the 0.16 library, resolves a queued row without clearing `focus`; a git merge of `backlog.toml` or a hand edit can do the same, and `validate` then refuses later writes to that row. `backlog focus --remove` accepts such a row, so the rank can always be cleared; After Merge reinstalls and restarts panes to keep the window short.

## After Merge

- `cargo install --path tomlctl` then `cargo install --path glimpse` (glimpse links tomlctl's library).
- Run the **predicted, unverified** datetime criterion by hand in a scratch repo: a fresh `tomlctl backlog add` row's `created` ends `Z`.
- Restart open glimpse panes so they pick up the new binary.

## Exploration Notes

### tomlctl backlog (`tomlctl/src/backlog/`)
- `schema.rs`: `FIELD_*` consts; `MINTED_FIELDS = [id, dedup_id, created, last_seen, seen_count]`; `pub MANAGED_FIELDS` (triage-owned, re-exported as `tomlctl::BACKLOG_MANAGED_FIELDS`); `KINDS`/`STATUSES` pinned by `vocabularies_are_pinned`; `kind` fail-soft (`coerce_kind`), `status` hard (`BacklogError::UnknownStatus`). Per-row `validate(&JsonValue) -> Result<(), BacklogError>` is where a tier enum / focus int check goes; doc-level invariants follow `validate_ids_unique(&TomlValue)`.
- `schema::clear_for_transition(table, target)` is the one point every transition passes (triage::rewrite, resolve_with_link → reconcile --apply, relate::dismiss); clears only `MANAGED_FIELDS`. Putting `focus` into `MANAGED_FIELDS` would clear it on promote/reopen too — needs a dedicated rule.
- Mint path: `add.rs build_request` → one `AddRequest.today` (`time::today_toml_date()`, UTC date) used for root `last_updated` AND row `created`/`last_seen` (`build_row`, `bump_row`); `add_many.rs parse_rows` same. `time.rs` has `now()` (jiff `Timestamp`, test-pinnable via `set_now_for_test`), `now_rfc3339()`; `inputs.rs` has a private `now() -> Datetime` (whole seconds, `Z`).
- Payload allow-lists: `add.rs CONSUMED_FIELDS` / `unknown_payload_key`; `edit.rs EDITABLE_FIELDS` (context, tags, evidence, origin, flow, base_sha); `struct Edit` has no CAS and no unset.
- `triage.rs`: `enum Transition {Promote, Dismiss, Resolve, Reopen}`, `triage_value(root, ids, &Transition, expect_status, …)` with `filter_on_status` CAS, `applied`/`skipped_stale`; batch `ROW_KEYS`.
- Library facade (`tomlctl/src/lib.rs`): `backlog_triage(root, ids, BacklogTriage, expect_status)`; `ledgers::classify` → `write_guarded(root, ledger, Vec<RowEdit{set, unset, expect}>, validate)` with JSON CAS (`items::stale_against`, `null` = absent) and hard-coded `StalePolicy::Skip`; `classify`/`transition` refuse backlog (`item_kind`). `ledgers::restore` allows only `restorable(LedgerRef::Backlog)` = `MANAGED_FIELDS` + status.
- Query: `backlog list` takes shared `QueryArgs` (`--sort-by F[:dir]`, `--where-gt/lt` ints); `apply_sort` stable; missing fields sort first ascending. Date predicates: `query.rs` equality on `ParsedRhs::Datetime` and `convert::compare_typed` compare `to_string()`; `DATE_KEYS`/`is_date_key` include created/last_seen. `--where created=<date>` will miss datetime rows.
- `compact.rs read_date` reads terminal dates (Datetime or string → `.date`), unaffected.
- CLI: `BacklogOp` in `tomlctl/src/cli/types/backlog.rs` → `backlog/dispatch.rs` (exhaustive destructure) → `backlog/mod.rs`. `FEATURES` in `cli/types/mod.rs`, mirrored in `tomlctl/tests/capabilities.rs` expected array and two `tomlctl/README.md` tables. `capabilities.rs ENUM_VALUES` maps clap id `tier` → A/B/C globally (DupTier) — a backlog `--tier` needs `SCOPED_ENUM_VALUES` + validator branch, or a `ValueEnum`. Read-only verbs listed in `read_only_subcommands_hide_write_integrity_flags_in_help` (`tests/capabilities.rs`).
- Tests: inline per file; integration `tomlctl/tests/backlog_{write,read,edit,reconcile,check_batch}.rs`, `library_facade.rs`, `capabilities.rs`.
- Phase 6 additions: no backlog reader outside `add.rs`/`edit.rs` names `FIELD_CREATED`/`FIELD_LAST_SEEN` (grep over `tomlctl/src`). tomlctl has no cargo features (`[features] default = []`), so clap types are always compiled. Version is `0.16.0` (`tomlctl/Cargo.toml`). The `/backlog` command's Step 0 gates on 0.13; backlog-clear's Step 0 runs `tomlctl backlog list --open`.

### glimpse (`glimpse/src/`)
- Read: `tomlctl::ledger_read` → JSON (`devalue_to_json`, TOML Datetime → string) → `ItemRow::backlog(lenient::<BacklogRow>(&raw))`; `lenient` drops type-mismatched fields. `ItemRow.raw` keeps file key order (preserve_order). File position is the index in `Tracking.rows`; `ItemRow` has no position field (adding one makes rows compare unequal on shift → clears `saving` in arrivals).
- Sort/group/filter live in `surface.rs` (ledger.rs module doc is stale). Backlog `groups() = [None, Kind, Area, Status]`, `sorts() = [Id, Newest, Kind, Status, Summary, Area]`; `compare(sort, dir, a, b)` — Newest = string compare, tie → `id_key`; `group_key(group,row) -> (u8, &str)`; `ItemsState::visible()` filter→sort→BTreeMap group. Keys `g` group, `S` sort, `D` dir. State persisted by `key()` in state.rs; `saved_keys_stay_fixed`, `group_and_sort_cycle_through_the_surface_options` pin keys; `cli.rs pick` derives names.
- Marks: `ItemsState.marks: BTreeSet<String>`, `toggle_mark`, `mark_visible`, `clear_marks`, `targets()` (marks else cursor).
- Details: `view/details.rs item_content(row, agents, inputs, theme, now)`, `fact(label, value, style)` closure; dates line at ~321 renders `created` raw. No backlog details test.
- Columns: `config.rs ItemColumn` (7 variants, `ALL`); `view/items.rs Columns::{measure,width}`, `title`, `value_cell`; zero-width columns hidden; saved `item_columns` replaces the default list.
- Writes: `actions::offered(Kind::Backlog, status)`; `transition_plan` → `WriteRequest::BacklogTriage`; bulk model `classify_plan` (group rows by identical `(fields, expect)`, per-row CAS from raw, one `WriteRequest::Classify` each); `RowUndo::of`; `Writes::dispatch` marks saving + pushes `UndoEntry`; undo → `actions::restore` → `ledger_restore_many`. `writer.rs WriteRequest` + `perform`.
- Keys: `Action` enum in app.rs; `keys::map`; legend `KEYS` + `every_bound_letter_key_has_a_legend_row` (hand list). Bound lowercase `h j k l o i z x f s a t d y v q g c m e u n r w`; uppercase `J K F Z H V S D C T`; `b` asserted unbound. Free uppercase: `A B E G I L M N O P Q R U W X Y`; free `< > , . ;`. "focus" name already used by diagram focus (`F`, `Action::CycleFocus`).
- `hook::parse_utc(&str) -> Option<u64>` parses `Z`/`±HH:MM` datetimes (None for bare date).
- Docs: README "Surfaces", "Writing" ("every backlog field but its triage status"), key table, `item_columns`; glimpse/CLAUDE.md "Control fields only".
- Phase 6 additions: `e` is already `Action::OpenClassify`, offered only on Review/Optimise (`app.rs`), so Backlog reuse needs no new key. A backlog transition's undo lists `BACKLOG_MANAGED_FIELDS` + `status` as touched (`actions::transition_plan`). glimpse CLAUDE.md: "A new editable field needs a facade verb that owns its validation, not a wider form." Legend `KEYS` has 38 entries today (`awk '/^const KEYS/,/^];/' glimpse/src/view/legend.rs | grep -c '^    ('`). `Purpose` (form purposes) lives in `actions.rs`. `glimpse/README.md` is 592 lines.

### Consumers and docs
- `claude/skills/backlog-clear/SKILL.md`: Step 0 `backlog list --open`; Step 1 eligibility (kind debt|annoyance default); Step 2 compares `created` date to base commit. `claude/commands/backlog-clear.md` thin carrier ("model routing" claim unbacked). `claude/commands/backlog.md` Step 2 dispositions; Step 0 version gate. `claude/skills/backlog-capture/SKILL.md` vocabularies (lines ~68-82), minting.
- `claude/skills/tomlctl/references/backlog.md` is at the 600-line ceiling (skills.rs gate) → new verb docs need a new reference file; `backlog-edit.md` editable-field table; `write.md` DATE_KEYS (`write_reference_enumerates_every_date_key`); `claude/skills/tomlctl/SKILL.md` verb + feature tables.
- Doc gates (`tomlctl/src/cli/dispatch/tests/`): `command_lint` (fenced tomlctl lines must parse), `flag_table_lint` (`| Flag |` under `` ## `verb` `` headings), `guidance_lint`, skills.rs size/link limits, `carrier_invokes_required_skills`.
- No lumina/scripts consumers of backlog dates. Fixtures with bare dates: glimpse `tests/fixtures/backlog.toml`, tomlctl tests (fine — readers keep accepting dates).
- `.github` mirror deleted in 1048839; dev-tools CLAUDE.md still describes it (out of scope).
- Commands: tomlctl `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast`, `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`, `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`; glimpse `cargo test --manifest-path glimpse/Cargo.toml`, `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`; fmt checks for both. After tomlctl change: `cargo install --path tomlctl` and `cargo install --path glimpse`; tomlctl version bump needs `glimpse/Cargo.lock` refreshed.

### Backlog
Live rows under the touched directories (`tomlctl backlog list --live --area-prefix …`):
- B-ec0c911b (debt, open) — backlog/schema.rs module header says the store carries no evidence field, but FIELD_EVIDENCE exists
- B-00b994ba (annoyance, open) — glimpse item table header titles cut short because Columns::measure ignores title widths
- B-be2797ef (debt, open) — glimpse legend Keys section ~40 rows no longer fits one screen
- B-317ed123, B-29b27f75, B-7ffd9652 (glimpse/src/diagram) — diagram concerns, unrelated to this plan
No seed flow. B-fa9da685 (direction, open) is this plan's own request record, not a fold-in candidate.

## Research Notes

Vet: Agent-1 (toml-datetime-clap) 4 sampled / 0 dropped / 0 downgraded; Agent-2 (focus-rank-design) 3 sampled / 0 dropped / 0 downgraded. Anchors re-read: `inputs.rs now()`, `query.rs` Datetime `to_string()` equality, `capabilities.rs` `("tier", ["A","B","C"])`, `items.rs` `StalePolicy::Abort` bail, `keys.rs` ALT early return. (No ledger exists in plan mode, so no `[[vet_events]]` append.)

### TOML datetimes (toml_datetime 1.1.2, jiff 0.2.38) — high
- An offset datetime with `second: Some(s)`, `nanosecond: None`, `offset: Some(Offset::Z)` displays and serialises as `2026-10-29T14:03:11Z`; `nanosecond: Some(0)` would append `.0`.
- `tomlctl/src/inputs.rs now()` already builds exactly this (`Timestamp::from_second(now().as_second())` → parse to `Datetime`); move it to `time.rs` as the shared minting helper (keeps `set_now_for_test` pinning).
- `Datetime` derives `Ord` field-wise (date, time with None < Some, offset): for bare dates and `Z` datetimes this is chronological, with bare date D before any `D T…Z`; the Display strings sort identically. Non-Z offsets would misorder — writers must only emit `Z`.
- jiff: truncate via `from_second(as_second())` (not `round(Unit::Second)`, which rounds half-expand); `Timestamp::from_str` rejects a bare date, so route 10-char strings through `civil::Date` as `time.rs parse_iso_to_date` does.
- Searched: installed registry sources, 2026-10-29.

### Query compat — high
- `query.rs` equality (`ParsedRhs::Datetime`, `TomlValue::Datetime(dt) => dt.to_string() == rhs`, `json_matches_toml`) is an exact string match: `--where created=2026-10-29` and `@date:` stop matching datetime rows. `--where-gt/lt` and `--sort-by` stay chronological for Z rows.

### clap ValueEnum for `--tier` — high
- A `#[derive(ValueEnum)]`-typed `--tier` is enumerated by `EnumValueParser::possible_values()`, which `capabilities.rs describe_values` consults before the global `ENUM_VALUES ("tier", A/B/C)` fallback — so capabilities reports lite/deep/human with no `capabilities.rs` edit. Do NOT add a `SCOPED_ENUM_VALUES` entry for it (`scoped_enum_values_name_real_args_clap_cannot_enumerate` fails). Pattern: `DupTier` `#[arg(long, value_enum, ignore_case = true)]` in `cli/types/items.rs`.

### Integer focus over JSON — high
- `toml_to_json` stores TOML ints via `JsonValue::from(i64)`; read `focus` with `as_i64()` and range-check so a negative value is rejected visibly, not read as absent.

### Focus-rank representation — sources high, recommendation medium
- Dense integer ranks; order by `(focus, id)`; display the 1..N position computed at read time, so gaps left by resolve/dismiss are harmless and never renumbered. A one-step move swaps two values (2 rows); insert-at-k shifts rows ≥ k down by one (≤ ~30 rows). LexoRank (Jira) and float sort orders (Linear `sortOrder: Float`) exist to avoid neighbour writes without a lock; here one exclusive lock exists and they are unreadable for `--focus 3`. Sparse gaps (10/20/30) clash with typed ranks.
- `--focus k` inserts at k and shifts; k > N clamps to N+1; unfocus removes the field and leaves the gap (GitHub Projects `updateProjectV2ItemPosition` uses relative `afterId`, implying the shift).
- A move must be all-or-nothing: `write_guarded` hard-codes `StalePolicy::Skip`, so a stale half of a swap would leave a duplicate rank. `compute_apply_mutation_with(…, StalePolicy::Abort)` exists and bails on any stale op.
- Searched: Atlassian "Managing LexoRank" (Server 11.0), Linear `schema.graphql`, GitHub GraphQL projects reference, taskwarrior.org/docs/urgency, todo.txt README — fetched 2026-10-29.

### Reorder key conventions — high (sources), untested in herdr/Windows Terminal
- lazygit binds Alt+Up/Down (and Ctrl+j/k) to move a commit; gitui, taskwarrior-tui and k9s have no reorder key. glimpse's `keys::map` returns `None` for every ALT chord (test `control_c_quits_and_other_chords_are_ignored` asserts `Alt+q` → None); `J`/`K` and `+`/`-` are taken. Ctrl+j is risky (LF in some terminals). `<`/`>` are free and need no modifier.
- Searched: lazygit `Keybindings_en.md`, gitui `key_list.rs`, taskwarrior-tui keybindings page, ranger `rc.conf` — 2026-10-29.
