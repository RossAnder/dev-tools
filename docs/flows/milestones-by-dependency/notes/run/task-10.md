Status: done — the lane check's digest and trail leave out every record of a milestone deferred at the window's open

```toml run-note
files_touched = ["tomlctl/src/plan/lane.rs"]

[[deviations]]
what = "A milestone counts as deferred at the open by the `old` value of its first in-window `elaboration` trail row, and is not counted when the first in-window trail row on it is its `*` add; with neither, its current state decides. The detail's wording was 'deferred now, plus any milestone with an elaboration change in the window'."
why = "The detail's union would also exclude a milestone flipped upfront to deferred inside the window, which was not deferred at the open; the task's constraint says such records are digested exactly as before. Reading `old` gives the state at the open exactly, from the same trail, with no run-row field."
means = "detail: deferred now plus milestones with an elaboration change stamped inside the window"

[[tangential]]
kind = "debt"
area = "tomlctl/src/plan/lane.rs"
summary = "A row removed from a deferred milestone during elaboration leaves a `*` tombstone trail row whose ref no live row carries, so trail_writes cannot attribute it to the deferred milestone and still reports it."
why = "An elaboration that drops a deferred row would show one false trail write; attributing it would need the tombstone to carry the row's milestone or a pre-window trail lookup."

[[tangential]]
kind = "question"
area = "tomlctl/src/plan/milestone.rs"
summary = "`plan milestone update --elaborated` writes its trail row only on an audited store, so on an unaudited store the flip leaves no trail and the elaborated milestone's records re-enter the digest at the check."
why = "The exclusion relies on the trail dating the flip; an unaudited v2 store would report the elaboration as a digest change."

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/lane.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "grep -q 'is_deferred\\|ELABORATION_DEFERRED' tomlctl/src/plan/lane.rs"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- plan::lane"
result = "pass (14 tests)"
```

## What changed

`tomlctl/src/plan/lane.rs`:

- New private `Deferred` set, built by `Deferred::at_open(store, started)` from the store and its trail alone: the milestones deferred when the window opened, and the refs of the rows whose `checkpoint` is one of them. Trail rows are keyed by the row's `ref` (`tasks add`/`update`/`remove`) or by `milestone:<id>` (`plan milestone add`/`update`), and `owns` matches both forms.
- `store_digest` drops those records from its projection before hashing: the rows, their specs, file notes, backlog links and finding links (through `Store::drop_ref_entries`), the milestones' `[[checkpoints]]` and `[[milestones]]` entries, and every trail row on any of them. The signature is unchanged. At the open no trail row falls inside the window, so the set is the deferred milestones, and at the check the first in-window `elaboration` row's `old` recovers the same set.
- `trail_writes` skips trail rows the deferred set owns, with the set computed at the run's `window_start`.
- Three unit tests. In the first, an audited elaboration adds a row with a spec, a file note and a backlog close, updates an existing deferred row's spec and flips the milestone to elaborated inside the window, and it leaves the digest unchanged with no trail write. In the second, a `files` write on a row of the running milestone, alongside that elaboration, is still reported in both the `rows` section and the trail. In the third, a milestone flipped upfront to deferred inside the window is still digested as before.

The gate-owned nextest accept check was not run.
