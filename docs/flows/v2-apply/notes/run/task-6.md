Status: done — `tomlctl review schedule <ids>|--all --flow <slug> --checkpoint <id>` mints linked fix tasks in a v2 flow's task store and marks the findings `scheduled`

## What changed

- `tomlctl/src/review/schedule.rs` (new): the verb.
  - **Refusal.** A flow whose directory has no `plan.toml` (`tasks::is_v2_flow`) is refused, and the error names the missing plan store.
  - **Selection.** `select::Selection::new` runs once for the code family and once for the perf family. Under `--all` the results merge in store order; with explicit ids they keep the order given. When every explicit id is refused, the verb errors the way export-v1 does and writes nothing. Under `--all`, an open code or perf `needs-plan` defect or cleanup appears in `refused` with selection's own reason; selection skips these silently.
  - **Folding.** Findings whose edited paths overlap, transitively, share one task. A finding's edited paths are its edit target plus `instances`, through `fields::edited_paths`.
  - **Rows.** Each row is built as designed:
    - `title` is the first summary, plus "(and N more)" on a folded task.
    - `files`, and `effort`: S, or M if any finding is medium.
    - `action` holds one `<id>: <summary>` line per finding. `detail` holds each finding's export-v1 description under its id.
    - `spec.outcome` is per finding: the failure scenario no longer happens, or the change is made.
    - Premises: `tomlctl review anchor <id> <store flag>` at `base`.
    - Accept checks: `tomlctl review anchor <id> <store flag> --gone`, `forward`, `writes_build = false`, for a `remove` cleanup. For everything else, `tomlctl review reproduce-status <id> <store flag> --expect gone`, `forward`, with no `writes_build` key.
    - The two constraints, and the executor rule.
    - `reason` is `apply <store path>: <ids>`. It is set on every row, and an audited store records it.
    - `review_store` and `fixes`.
  - **Writes.** Rows are minted under the task store's lock through `add-many`'s own parse and append. Ids are predicted from `next_id` so `depends_on` becomes `needs`, both within the batch and onto an earlier live linked task. A dependency with no task is reported in `dropped_deps`. If the predicted ids differ from what was minted, the write is refused.
  - **Order of writes.** The task store is written before the review store. On a rerun, a finding already in a live task's `[[finding_links]]` `fixes` is marked without a second mint. A live task is one whose row exists, has no `commit` and is not `deferred`. A link to a committed task gets a new task. When nothing needs minting, the task store is not written.
  - **Output.** `{ok, flow, review_store, scheduled[{finding, task, ref, action: minted|linked}], settle: [], held, refused, dropped_deps, skipped_stale}`.
- `tomlctl/src/tasks/add.rs`:
  - `add_many`'s row parse is factored into `parse_rows`, with behaviour unchanged.
  - New `Store::add_many_locked(&mut self, ndjson)`, which runs the same parse and `append` over a store the caller already holds inside `store::mutate`.
- `tomlctl/src/review/export.rs`: `description` is now `pub(super)` so schedule composes the same text. export-v1's output does not change.
- `tomlctl/src/cli/types/review/export.rs`: `ReviewScheduleArgs`, with ids or `--all`, the store group, `--flow`, `--checkpoint` and the write integrity flags.
- `tomlctl/src/cli/types/review/mod.rs`: the `ReviewOp::Schedule` variant.
- `tomlctl/src/review/dispatch.rs`: routes `Schedule`.
- `tomlctl/src/review/mod.rs`: `mod schedule;`.
- `tomlctl/tests/review_schedule.rs` (new): ten tests:
  - designed rows for a cleanup and a defect;
  - folding;
  - a medium defect runs on deep;
  - selection refusals with both stores byte-unchanged, and `--all` listing large and needs-plan findings as refused;
  - `emitted_checks_parse`, which runs every emitted premise and accept command and expects exit 0 or 1 with a JSON object;
  - `a_v1_flow_with_no_plan_store_is_refused`;
  - `an_approved_reason_is_stamped_on_every_row`;
  - `depends_on_becomes_needs`;
  - two `rerun` tests: linked without a mint, and committed-then-reopened mints a new task.

## Deviations and choices

- **Reaching add.rs from the review module.** `tasks/mod.rs` declares `mod add;` privately and is not in this task's files. To use `add-many`'s path I added an inherent `impl Store` method in add.rs (`add_many_locked`). An inherent method's `pub(crate)` visibility does not depend on the module that holds the impl, so `crate::tasks::Store` (already re-exported) exposes it.
  - Alternative: return `failed` to get `tasks/mod.rs` added. Rejected, because the outcome is reachable inside the files.
  - Alternative: shell NDJSON into `tasks::dispatch`. Rejected: it prints to stdout and cannot compose rows under the lock, and the rerun check and the predicted `needs` ids need that.
- **Passing fields to `schedule::run`.** The CLI args are not passed as a struct. A small `Request` carries the fields, as anchor and reproduce already do. The reason is the same: `crate::cli` re-exports review arg types through an explicit list in `cli/mod.rs`, which this task does not hold.
- **A `remove` cleanup with no `old`** gets the reproduce-status accept check, not `anchor --gone`, because `review anchor --gone` refuses such a finding (see the anchor verb's refusals).
- **Refusal wording.** Selection is called per family because `select.rs` takes one destination family and is not in this task's files. Explicit ids are partitioned by the finding's own family, so the "does not belong in a ... ledger" refusal never fires. For a plan-family or advisory id, the refusal text is select.rs's, which still mentions a v1 ledger or export.
- `settle` is printed as an empty array, so the output shape is stable before settle tasks land.

## Handover

None.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean (one complex-type warning fixed by returning `Selection`).
- `rustfmt --edition 2024 --check` on `schedule.rs`, `export.rs`, `dispatch.rs`, `cli/types/review/export.rs`, `tasks/add.rs` and `tests/review_schedule.rs`: clean after applying it. `tests/common` was untouched by that run. The two `mod.rs` files declare `mod` children and are left to the gate.
- Accept "the schedule module exists" (`test -f tomlctl/src/review/schedule.rs`): pass.
- The four `cargo nextest ... binary(review_schedule)` accept checks have `Writes build: yes`, so the gate runs them. They were compiled by clippy `--all-targets` but not run. The filters match test names: `emitted_checks_parse`, `v1_flow`, `approved_reason`, `depends_on`, `rerun`.

TANGENTIAL: debt | tomlctl/src/review/select.rs | the refusal text for plan-family and advisory findings names a v1 ledger and export, which reads oddly from review schedule | users of schedule see export-v1 wording; a verb-neutral phrasing would serve both callers
TANGENTIAL: debt | tomlctl/src/tasks/mod.rs | `add` is a private module, so other verb groups reach add-many only through the inherent `Store::add_many_locked` | a `pub(crate) use` there would make the entry point explicit
TANGENTIAL: debt | claude/skills/tomlctl/references/review.md | the review flag reference does not document `review schedule` | agents learn verbs from that reference
