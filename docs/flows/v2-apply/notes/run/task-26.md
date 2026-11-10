Status: done — review triage presents blocked findings by `blocked_on`, records decision answers, routes design to /plan-v2 and forces overdue proposals to a disposition

## What changed

- `claude/skills/review-v2-engine/references/triage-and-close.md`
  - Stage 7 presentation: the Needs-a-plan group is replaced by the blocked findings grouped by `blocked_on` (read with `review list --status open --blocked-on any`) and the deferred proposals from the digest's `proposals`, with age and overdue flag.
  - New **Blocked findings** block with per-kind handling: `decision` asked together in one `AskUserQuestion` round (four questions per call, calls back to back) and each answer staged and recorded with `review triage <id> --answer-file <path> --expect-status open`; `evidence` offered for apply as a settle task; `measurement`/`alternatives` listed as waiting for trials; `design` offered the `plan` route naming `/plan-v2`, user-picked only. It says outright that no route is taken on the orchestrator's own recommendation.
  - New **Deferred proposals** block: listed with age; an overdue one needs a disposition (`review settle --accept`/`--refute`, or a triage move off `open`) before `review run finish`, which refuses even with `--incomplete-file`.
  - The gate: a headless/empty answer records no answers and leaves blocked findings untouched; with an overdue proposal the run is left open, step 5 is skipped and the overdue ids are named.
  - Grammar: new `answer`, `accept`, `refute` rows; `plan F5` now names `/plan-v2` with no store write (it used to mint a backlog row for `/plan-new`).
  - Convert payload loses `fix_shape`.
  - Stage 8 step 2 splits plan picks (name `/plan-v2 <summary> — planned from tomlctl review show <id> … --for implementer`, finding stays open and blocked on design) from backlog picks. Step 3's refusal list names the blocks instead of `needs-plan` and says evidence picks become settle tasks and `apply all` skips blocked findings except evidence settle tasks. Step 5 names the overdue refusal. Step 7's close summary lists answers, plan picks and proposal dispositions.
  - The `--<flag>-file` twin list gains `--answer-file` and `--fix-file`.
- `claude/skills/review-v2-engine/SKILL.md`: stage 0 capability check swaps `review_list_fix_shape` for `review_blocked_on`, `review_settle`, `review_triage_answer`; step 5 lists open blocked findings with `--blocked-on any`; stages 6-8 summary names the blocked groups, deferred proposals, the one decision round and overdue dispositions.
- `claude/skills/review-v2-engine/references/plan-flow.md`: section 1 capability check makes the same swap; step 3 says "open blocked findings".
- `claude/commands/review-v2.md`: stage 8 hand-off refusal sentence names blocks other than `evidence`, and that an evidence pick becomes a settle task.

The `/apply-v2` hand-off (stage 8 step 4) is untouched.

## Deviations

None from a `suggested` means. Choices made where the detail was silent:
- A `plan` pick writes nothing to the store; the finding stays open and blocked on design. Alternatives were parking it (a status move the design does not call for) or keeping the old backlog row (which the design replaces with naming /plan-v2). Leaving it open keeps it visible in the next run's opening listing.
- `AskUserQuestion` takes at most four questions and four options per call, so "one round" is written as back-to-back calls, and decisions with more than four options take the answer as text.

## Checks

- Every flag used was checked against the source (`tomlctl/src/cli/types/review/{triage,read}.rs`, `cli/types/mod.rs` features) and `claude/skills/tomlctl/references/review*.md`. The installed binary (0.23.0) predates `settle`/`--answer-file`/`--blocked-on`; the plan's earlier task bumps it, so a `cargo install --path tomlctl` is needed before running these steps.
- Accept 1 (no fix-shape/needs-plan wording in the four files): PASS.
- Accept 2 (`--answer-file` and `/plan-v2` in triage-and-close.md): PASS.
- No compile check: markdown only. The `cli::dispatch::tests` carrier/flag gates build, so they are the gate's.

TANGENTIAL: none
