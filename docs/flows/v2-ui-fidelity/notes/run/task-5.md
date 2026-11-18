Status: done — accept entries take run_by = "look" with a `states` list, validated, round-tripped and stamping version 3

```toml run-note
files_touched = [
  "tomlctl/src/tasks/schema.rs",
  "tomlctl/src/tasks/add.rs",
  "tomlctl/src/tasks/show.rs",
  "tomlctl/src/tasks/update.rs",
  "tomlctl/src/plan/store.rs",
  "tomlctl/tests/tasks_spec_add.rs",
]

[[tangential]]
kind = "debt"
area = "tomlctl/src/tasks/snapshot.rs"
summary = "The snapshot's accept JSON writes run_by and covered_by on every entry but not the new `states` list."
why = "glimpse and any snapshot reader will not see which capture states a look entry is judged on."

[[tangential]]
kind = "question"
area = "tomlctl/src/plan/store.rs"
summary = "entry_matches compares the string fields and writes_build of a written accept entry but not `states`."
why = "Two look entries differing only in states would both match a review edit's entry; harmless today, worth a look when plan review targets look entries."

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/tasks/schema.rs tomlctl/src/tasks/add.rs tomlctl/src/tasks/update.rs tomlctl/src/tasks/show.rs tomlctl/src/plan/store.rs tomlctl/tests/tasks_spec_add.rs"
result = "pass"

[[checks]]
command = "grep -q '\"look\"' tomlctl/src/tasks/schema.rs && grep -q '\"states\"' tomlctl/src/tasks/add.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/accept_look/)'"
result = "not-run"
```

## What changed

- `tomlctl/src/tasks/schema.rs`: new `RUN_BY_LOOK = "look"` joins `RUN_BY_OWNERS` (now four). `Accept` gains
  `states: Vec<String>`. `owner_problem` refuses a look entry with no states or a non-empty check, and refuses
  `states` on any other owner, an unset `run_by` included. `accept_from_toml` reads `states`, and `accept_to_toml`
  writes it only when non-empty, so existing entries keep their bytes. `holds_v3_keys` (task 4's predicate) also fires
  on a keyed spec whose accept entry has `run_by = "look"` or non-empty states. The `SCHEMA_VERSION` doc names the
  new keys. Unit tests `accept_look_needs_states_and_an_empty_check`, `accept_look_states_are_refused_on_every_other_owner`
  and `accept_look_round_trips_and_raises_the_stamp_to_version_3`; the existing literals get `states: Vec::new()`.
- `tomlctl/src/tasks/add.rs`: `ACCEPT_FIELDS` gains `"states"`, and `accept_entry` reads it with `strings`.
  Validation goes through the shared `owner_problem`.
- `tomlctl/src/tasks/show.rs`: `accept_json` prints `states` when set. `update.rs`'s review-edit `expect`
  comparison goes through the same function, so a look entry's verbatim copy still matches.
- `tomlctl/src/tasks/update.rs`: the test `accept` literal only.
- `tomlctl/src/plan/store.rs`: the `#[cfg(test)]` `Accept` literal only, with `states: Vec::new()`, as the
  constraint allows.
- `tomlctl/tests/tasks_spec_add.rs`: four `accept_look_*` integration tests:
  - a look entry round-trips through `add-many` and `tasks show` and stamps the store at version 3;
  - missing states or a non-empty check is refused, with the store bytes untouched;
  - states on a gate owner or an unset owner is refused;
  - `plan probes --emit` gives a look entry no probe line, even with `writes_build = false`, and a guard beside it
    records `guard-alone`, which proves the look entry never counts as discriminating. `probes.rs` is unchanged.

A look entry's `kind` keeps the existing vocabulary. Because its check is empty, the existing check-less rule also lets
the kind be empty.

The accept check `cargo nextest run ... -E 'test(/accept_look/)'` is marked `Run by: gate`, so I did not run it. The
new tests compile under clippy `--all-targets`. The probe test's emit and capture line shapes come from
`probes.rs` `accept_lines` and the helpers in `tests/plan_probes.rs`.
