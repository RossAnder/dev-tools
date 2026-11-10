Status: done — `tomlctl review reconcile <store flag> --flow <slug>` moves scheduled findings by their task's state in the apply flow

## What changed

- `tomlctl/src/review/reconcile.rs` (new): the verb.
  - It starts from the review store's `scheduled` findings whose `apply_flow` is the flow, not from the flow's links.
  - Each finding's task is the row whose `ref` is the finding's `apply_task`.
  - When no row carries that ref, it reads the flow's `[[finding_links]]`. It looks for links of this store whose `fixes` or `settles` name the finding and whose ref names a row, and takes the last one. That row is treated as the retitled task, and `apply_task` is rewritten to its ref.
  - Whether the task fixes or settles the finding is read from `[[finding_links]]`.
  - Transitions:
    - done with a non-empty `commit`, fixing: `resolved`. `resolution` is `<flow> <ref> <commit>` and `resolved` is today. `apply_flow` and `apply_task` are kept.
    - done with a commit, settling: back to `open`.
    - `deferred`, or no row at all: back to `open`.
    - pending, in-progress, failed, parked, or done without a commit: stays scheduled and is listed in `in_flight`.
  - Every return to `open` sets `reopen_reason` starting with `apply: ` and removes `apply_flow` and `apply_task`.
  - The task store is only read. The review store is written once, under its lock, and only when something moved (a resolve, a reopen or a retitle). The move is first computed on a copy, as `sync-v1` does.
  - If a finding is scheduled into the flow but the flow has no `tasks.toml`, the verb refuses. Otherwise every finding would be reopened as removed because of a mistyped slug.
  - A missing review store prints empty lists.
  - Output: `{ok, flow, review_store, resolved[{finding, task, commit}], reopened[{finding, task, reason}], in_flight[{finding, task, status}], retitled[{finding, from, to}]}`.
- `tomlctl/src/cli/types/review/export.rs`: `ReviewReconcileArgs` (store group, `--flow`, write integrity flags). The module doc names the new verb.
- `tomlctl/src/cli/types/review/mod.rs`: the `ReviewOp::Reconcile` variant.
- `tomlctl/src/review/dispatch.rs`: routes `Reconcile`.
- `tomlctl/src/review/mod.rs`: `mod reconcile;`.
- `tomlctl/tests/review_reconcile.rs` (new): six tests.
  - A commit resolves the finding and keeps the link.
  - Done-without-commit, failed, parked and pending all stay scheduled, and the store is byte-unchanged.
  - Removed and deferred rows reopen the finding with `apply: ` and the link cleared.
  - A committed settle task reopens its finding.
  - `a_retitled_task_is_followed_and_apply_task_rewritten`.
  - Findings scheduled into another flow are untouched.

## Deviations and choices

- **`retitled` output key.** The output adds a `retitled` list beside the designed `resolved`, `reopened` and `in_flight`, so a rewrite of `apply_task` is visible. Each retitled finding still appears in one of the three designed lists.
- **Choosing among links when the ref names no row.** I weighed two options:
  - the first matching link;
  - the last matching link.

  I chose the last, because `review schedule` appends a reschedule's link after the earlier task's. See the tangential item below for the case neither choice can tell apart.
- **`dispatch.rs` passes fields, not the args struct.** `reconcile::run` takes the target, the flow and the integrity flags. `crate::cli` re-exports review arg types through an explicit list in `cli/mod.rs`, which this task does not hold. Schedule and anchor are routed the same way.
- **Missing task store.** Refusing a flow with no `tasks.toml` is my addition, not in the design. It guards against a mistyped slug mass-reopening findings.

## Handover

None.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean. This compiled the new test binary.
- `rustfmt --edition 2024 --check` on `reconcile.rs`, `tests/review_reconcile.rs`, `dispatch.rs` and `cli/types/review/export.rs`: clean after applying it. The two `mod.rs` files declare `mod` children and are left to the gate.
- Accept "the reconcile module exists" (`test -f tomlctl/src/review/reconcile.rs`): pass.
- C1's grep for `finding_links` in `reconcile.rs`: pass.
- The two `cargo nextest ... binary(review_reconcile)` accept checks are `Writes build: yes`, so the gate runs them; they were compiled but not run here. The `test(retitle)` filter matches `a_retitled_task_is_followed_and_apply_task_rewritten`.

TANGENTIAL: bug | tomlctl/src/review/reconcile.rs | a finding fixed, committed, reopened and rescheduled whose new task row is then removed still has the old committed task's link, so reconcile follows that link as a retitle and resolves it | links carry no round or schedule marker to tell an old committed link from a retitled live one; a marker on `[[finding_links]]` (or dropping a finding from a committed link when schedule mints anew) would settle it
TANGENTIAL: debt | claude/skills/tomlctl/references/review.md | the review flag reference documents neither `review schedule` nor `review reconcile` | agents learn verbs from that reference
