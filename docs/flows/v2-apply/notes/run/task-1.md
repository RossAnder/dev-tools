Status: done — the task store carries `[[finding_links]]`, written by `tasks add-many`, re-keyed by `--ref`, dropped with its row

## What changed

- `tomlctl/src/tasks/schema.rs`: a new `FindingLink { ref, store, fixes, settles }` and a `Store::finding_links` field. They are read and written like `[[backlog_links]]`. An entry with no `ref`, or with neither `fixes` nor `settles`, is dropped on read and on write, and the array is omitted while empty. `finding_links` counts as a v2 key: it is in `holds_v2_keys` and in the `to_toml` stamp, and it is written after `[[backlog_links]]` and before `[[specs]]`. `drop_ref_entries` drops it too. The unit fixtures are extended: the v2 fixture carries a link, `each_v2_key_alone_raises_the_stamp` covers `finding_links`, and the emptied-store test asserts the key is omitted.
- `tomlctl/src/tasks/add.rs`: `add-many` accepts the row keys `review_store`, `fixes` and `settles` (`ROW_FIELDS` 16 to 19). `RowExtras` carries an `Option<FindingLink>`, and `land` pushes it under the minted ref. `finding_link_from_row` refuses finding ids without a non-blank `review_store` and refuses a blank id. A `review_store` with no ids links nothing and stores nothing.
- `tomlctl/src/tasks/update.rs`: the `--ref` rename re-keys the row's finding links, the same way it re-keys backlog links.
- `tomlctl/tests/tasks_finding_links.rs` (new): black-box tests covering:
  - the link lands under the minted ref and the store is stamped 2
  - an empty `settles` is written as `[]`
  - the link survives an unrelated write unchanged
  - a `--ref` rename carries the link
  - `tasks remove` drops only that row's link, and the array goes once empty
  - a store linking nothing writes no key and stays at version 1
  - a stored entry naming no finding is dropped
  - refusals by line with the store bytes untouched: ids without a store, a blank store, a blank id, wrong JSON types
- `claude/skills/tomlctl/references/tasks-store.md`: documents `[[finding_links]]` beside `[[backlog_links]]`, including the example, its v2-key status, key order and lifecycle. The `schema_version` row and the store-shape paragraph now list it.
- `claude/skills/tomlctl/references/tasks-write.md`: under `tasks add-many`, the three row keys are in the accepted set and the key table. The `tasks update --ref` and `tasks remove` paragraphs name finding links.

`[[backlog_links]]` is untouched in code and in behaviour.

## Deviations and choices

- Made `finding_links` a v2 key: it raises `schema_version` to 2 and makes `tasks remove` treat the store as v2. The alternative was to leave it unstamped, like `backlog_links` and `minted_refs`. I rejected that for two reasons:
  - On a v1 flow, `tasks remove` runs `drop_ref_entries` only for v2 stores (`tomlctl/src/tasks/remove.rs`, `retire`), and remove.rs is outside this task's files. Unlike backlog links, no import rebuilds finding links. So "removing the row removes its link" would not hold on a v1-stamped store.
  - A binary that only knows version 1 would silently drop the links on its next write. With the stamp it refuses the store instead.
  
  A store with no finding links is stamped exactly as before.
- The detail says a title change "re-derives the row's ref". In the code, `ref` moves only through `--ref` (the `patch` function in update.rs refuses a retitle that moves the derived ref without `--ref`). The carry is therefore in the `--ref` rename path, and that is what the test exercises with `--set title=… --ref …`.
- Refusal policy, which the design did not state: finding ids without a `review_store` are refused rather than dropped. This follows the existing "refuse rather than silently lose" idiom used for blank file notes. A `review_store` alone is accepted and stores nothing, per "An entry with no `fixes` and no `settles` is not stored".
- `tasks update --ndjson` (`patch_links`) does not take `fixes`/`settles`/`review_store`. Only `add-many` was in the action.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean, with no warnings. This compiles the new test binary too.
- `rustfmt --edition 2024 --check` on schema.rs, add.rs, update.rs and the new test file: clean. Two import lines were reflowed by hand. rustfmt was not applied to the test file because its `mod common;` would recurse.
- Accept `grep -q 'finding_links' tomlctl/src/tasks/schema.rs`: pass.
- Accept `grep -q 'finding_links' claude/skills/tomlctl/references/tasks-store.md`: pass.
- Accept `cargo nextest run … -E 'binary(tasks_finding_links)'`: Writes build: yes, so not run; the gate runs it. The schema.rs unit tests I extended are covered by the gate's suite.

## Handover

None.

TANGENTIAL: question | tomlctl/src/tasks/update.rs | `tasks update --ndjson` can replace a row's `closes`/`refs` but has no way to edit a row's `fixes`/`settles` | review schedule or reconcile may need to amend a link after minting, which today needs remove-and-re-add
