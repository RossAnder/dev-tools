# Plan: backlog triage and findability — pickup tier, focus queue, datetimes, full-field glimpse surface

**Plan path**: `docs/plans/giggly-conjuring-brook.md`
**Created**: 2026-10-29

> PAUSED at /plan-new Phase 6 at the user's request ("store this all in a backlog item instead of planning now; we resume tomorrow"). Captured as backlog item B-fa9da685, whose context points at this file. This document keeps Phases 1–5 (exploration, research, decisions), so resuming `/plan-new` starts at Phase 6 design. No flow created yet.

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
No seed flow.

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
