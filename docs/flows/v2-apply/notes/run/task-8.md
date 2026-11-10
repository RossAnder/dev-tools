Status: done — `plan view --for worker --task <n>` prints a `## Findings` block for each finding its `[[finding_links]]` name, read from the review store at view time in the implementer projection

## What changed

- `tomlctl/src/review/store.rs`: new `pub(crate) fn implementer_findings(path, ids, integrity) -> Result<Vec<(String, Option<Row>)>>`. It loads the store with `load`, which honours `--verify-integrity` and errors on a missing store. It then projects each id through `View::project(Role::Implementer, …)`, in the order given, with `None` for an id the store does not hold. The `Row` alias is widened from `pub(super)` to `pub(crate)` so the signature can name it; clippy's `type_complexity` flagged the spelled-out type.
- `tomlctl/src/review/mod.rs`: `pub(crate) use store::implementer_findings;` re-exports it. This is the one crate-visible entry point; `store` and `fields` stay private.
- `tomlctl/src/plan/view.rs`: `worker` now takes the read-integrity args. A new `findings_block` runs after the `## Task` block and before `## Design`.
  - It runs only for `[[finding_links]]` entries whose `ref` is the row's and that name at least one finding. With no such entry it prints nothing, so an unlinked task's view is byte-identical to before.
  - For each link it resolves `store` against `repo_or_cwd_root()`.
  - Each finding prints as `### \`<id>\` (fixes|settles), from \`<store>\``, with every implementer-projected field except `id` as `**key:** value`. Strings print verbatim; other values print as compact JSON.
  - A store that cannot be read, or a root that cannot be resolved, prints one line: `_Review store \`<store>\` cannot be read: <error>._`. An id the store lacks prints one line: `_Finding \`<id>\` (<role>) is not in \`<store>\`._`. The view never fails on a link.
- `tomlctl/tests/plan_view.rs`: a review-store fixture plus five tests, every name containing `finding`:
  - a linked task shows each finding it fixes or settles, with the fields outside the implementer projection (lens, status, route, cell) absent;
  - the text is read at view time (rewriting the store changes the view);
  - an absent store prints one line;
  - a missing finding prints exactly one line;
  - the view of another, unlinked task in the same flow equals the standard fixture's.

## Deviations and choices

- Placement: the findings block comes after the task block and before the design sections. Neither the detail nor the design says where it goes. I put it next to the row it belongs to, and before the design, which is reference material.
- Rendering is generic over the projected map (in key order, since serde_json has `preserve_order`) rather than a fixed field list. A fixed list would re-implement the projection in the plan module, which the detail forbids, and it would go stale whenever `fields.rs` changes visibility.
- Integrity: the view's `--verify-integrity` also applies to each linked review store. A mismatch there becomes the one "cannot be read" line, not a failed view, as the constraint requires.
- The store path is joined to the repo root without confining it under `.claude/`. The view is read-only, and the paths come from the flow's own task store, which `review schedule` writes.

## Handover

None.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean after the `Row` alias change. This compiles the extended test binary.
- `rustfmt --edition 2024 --check` on `src/plan/view.rs`, `src/review/store.rs` and `tests/plan_view.rs`: clean. `src/review/mod.rs` declares `mod` children and is left to the gate.
- Accept "the worker view reads finding links" (`grep -q 'finding_links' tomlctl/src/plan/view.rs`): pass.
- Accept `binary(plan_view) & test(finding)` and `binary(plan_view)`: `Writes build: yes`, so the gate runs them. They compile but were not run here.

TANGENTIAL: none
