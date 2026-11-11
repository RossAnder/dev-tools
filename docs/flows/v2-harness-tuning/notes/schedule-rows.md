Verdict: A minted row's files are only the finding's edited paths (`target_file` or `file`, plus cleanup `instances`; tomlctl/src/review/schedule.rs:381 via store.rs:406), its needs only `depends_on` (schedule.rs:500-521), and its accept is one `reproduce-status <F>` per finding (schedule.rs:670, 709-730). No stored field names a defect's other files or test files; they exist only as free text in `fix`/`evidence`. Holder needs and the label change fit inside schedule.rs and view.rs. Extra files need a new finding field, and the aggregate check needs `reproduce-status` to select a milestone.

## Answer

- **files.** `Compose::row` (schedule.rs:608-618) unions `edited(f)` (schedule.rs:381-390), which is `Finding::edited_paths` (store.rs:406-419) → `fields::edited_paths` (fields.rs:1388-1394): `target_file` if set, else `file`, then every `instances` path. `fold` (schedule.rs:394-435) groups findings by those same paths. Nothing else contributes.
- **needs.** `Compose::plan` (schedule.rs:495-521) maps each finding's `depends_on` to the task fixing that dependency. No file holder is consulted. drain.md:197 says "`needs` follows the files" for minted rows, but schedule does not apply that rule.
- **accept.** `Compose::accept` (schedule.rs:709-730) adds one entry per finding: `review anchor <F> --gone` with `writes_build: false` for a `remove` cleanup with `old`, and otherwise `review reproduce-status <F> <store-flag> --expect gone` with no `writes_build`. The schema reads an absent `writes_build` as true (tasks/schema.rs:278-280), so the worker view prints `Writes build: yes` (plan/view.rs:740-741).
- **Fields that name other files.** For a defect, only `file`, `line` and `symbol` locate code. `fix` (fields.rs:642, ≤800 chars), `evidence` (fields.rs:657, ≤5 strings) and `failure_scenario` are free text. `target_file` and `instances` are cleanup-only (fields.rs:674, 681; the test at fields.rs:2079-2083 shows the path check refusing a bad `instances`). No field anywhere names test files. F87/F92/F97/F131 named theirs only in prose.

## What each change takes

1. **Extra files and test files (B-7b9d8057), needs-plan.** Add a finding field, for example `fix_files` (StrList, `PathForm::Path`, `DEFECT_OR_CLEANUP`, capped). That means a catalogue row in fields.rs near 642-684, a row in the role-projection matrix (fields.rs ~1718-1780), a line in `review-v2-method` SKILL.md's field table (~line 76), and finder and verifier prose telling them to fill it. Then union it in schedule's `edited()` only. **Do not** add it to `Finding::edited_paths`: `select.rs:180` uses that to hold back test removals, and `fields::route` (fields.rs:1426) uses it to send a cleanup to the verifier. A listed test file would change both. Two knock-ons to decide:
   - The lite rule (`files.len() <= 2`, schedule.rs:671) would push more rows to `deep`.
   - A new test file needs `file_notes` `(new)` so the view reads it as new (view.rs:711-727).

   Stores written before the field have only `fix` prose. A fallback would have to parse backticked paths out of `fix`/`edit`/`fix_sketch`, which is heuristic.
2. **needs on the latest done holder (B-e2b98a71).** `Compose::plan` already gets `tasks: &crate::tasks::Store`, and it runs again under the lock (schedule.rs:156-157), so the result stays consistent. For each path in a pending row's files, take the max `id` among `tasks.items` whose `files` contain the path and whose `status` is `Done`, then push it into `needs` with the existing dedupe. Rows minted in one call are already file-disjoint because `fold` merges overlaps. A done row's dependency is already satisfied, so readiness does not change. The warning comes from `Graph::overlap_pairs` (tasks/graph.rs:340-360), which includes every row whatever its status, through `overlap_findings` (tasks/check.rs:639-655).
   - The edge removes only the new-row↔holder pairs. Warnings between older unchained holders remain.
   - A *pending* holder still warns. drain.md:197 takes the highest id of any status, so "done only" is narrower than the engine's own rule and is a decision for the planner. A pending holder in a later checkpoint would produce a cut error.
3. **One aggregate reproduce check per milestone (B-3a80ce63).** `reproduce-status` already takes several ids and exits 0 only if every one matches (reproduce.rs:242-278). It reads each finding's latest reproduction regardless of flow (reproduce.rs:263).
   - **Route A.** apply-v2-engine step 5 adds `--gate 'tomlctl review reproduce-status <ids> <store-flag> --expect gone'` to the fix milestone. This misses findings the settle drain later schedules into `apply-<n>` (apply-v2-engine SKILL.md:173).
   - **Route B (robust).** Add a selector such as `--flow <slug> --milestone <id>` to `reproduce-status`, resolving the ids from the task store's `[[finding_links]]` on rows with that `checkpoint`. Schedule (schedule.rs:721-727) and the reproduce fix tasks (findings.md:160) would then emit that same command string on every row. drain.md:247 already drops an accept check identical to one already listed, so the gate runs it once. findings.md:172's guarantee would need rewording.

   Either route touches the per-row accept, and every fix row still needs at least one accept with a check (drain.md:195).
4. **Worker view marks the gate's check (B-9728b95d).** The decision is already in the data: `writes_build` true means the gate runs the check (drain.md:237; agents plan-worker-{lite,deep}.md:52). Only the label is misleading, since `reproduce-status` builds nothing. Change view.rs:740-741 to say who runs the check, for example `Run by: gate|worker`. Sites that move with it:
   - tests/plan_view.rs:778, 786, 799-800
   - plan.md:341-342
   - tomlctl/README.md:569
   - both agents' lines 25 and 52
   - the capability token `plan_view_writes_build` (cli/types/mod.rs:182), which /implement-v2 and /apply-v2 pre-flights probe (commands/implement-v2.md:29, apply-v2.md:31). A rename needs a new token, or the old one kept.

   The alternative is a worker-runnable accept beside it. None exists generically for a defect: `anchor --gone` fits only `remove` cleanups.

## Findings

- **Library/API or file**: tomlctl/src/review/schedule.rs:381 `edited` / store.rs:406 `Finding::edited_paths`
- **Source**: file read; fields.rs:1388-1394
- **Evidence-grade**: high
- **Finding**: a fix row's files are only `target_file`/`file` and cleanup `instances`, so a defect whose fix also edits another file or a test file mints a row that forbids touching it.
- **Details**: `Compose::row` (schedule.rs:608-618) unions `edited()` and adds a constraint "Touch no file outside this task's files" (schedule.rs:645). `instances`/`target_file` are `kinds(KindSet::CLEANUP)` (fields.rs:674-684), so a defect cannot list extra paths at all.
- **Counter**: a defect finding row carrying `instances` that `review add` accepts would refute the cleanup-only claim. The catalogue's `.kinds(KindSet::CLEANUP)` says otherwise.
- **Impact on plan / report**: needs a new finding field plus producer prose (needs-plan). Keep it out of `edited_paths` because of select.rs:180 and fields.rs:1426.

- **Library/API or file**: tomlctl/src/review/schedule.rs:495-521 `Compose::plan` needs loop
- **Source**: file read; claude/skills/implement-v2-engine/references/drain.md:197; tomlctl/src/tasks/graph.rs:340-360
- **Evidence-grade**: high
- **Finding**: `needs` comes only from `depends_on`. Adding the max done holder id per file in `plan()` clears the new-row `dag/unreachable-claim` pairs.
- **Details**: `overlap_pairs` checks every row with no status filter, so committed rows count. `plan()` has the task store and is re-run under `mutate_store`. The test anchor is tests/review_schedule.rs:594 `depends_on_becomes_needs`, for a sibling case.
- **Counter**: if `nodes_of` (tasks/check.rs:391) filtered out done rows, the warnings would not come from done holders. It does not filter them; the call passes `&store.items`.
- **Impact on plan / report**: one-file change plus a test. The planner decides done-only versus any-status, the latter matching drain.md:197.

- **Library/API or file**: tomlctl/src/review/schedule.rs:709-730 `Compose::accept`; tomlctl/src/review/reproduce.rs:230-278 `status`
- **Source**: file read; drain.md:231-247; findings.md:160, 172
- **Evidence-grade**: high
- **Finding**: per-finding `reproduce-status` accepts are what send N commands to the gate. A milestone-scoped selector, emitted as one identical string, collapses them to one through the gate's existing dedupe.
- **Details**: `status` takes an id list and ANDs the matches, and has no flow or milestone selector. drain.md:247 already drops a duplicate check. findings.md:160 mints the same per-finding shape for reproduce fix rows.
- **Counter**: if the gate prompt did not dedupe identical commands, route B would still send N copies. drain.md:247 says it does.
- **Impact on plan / report**: needs-plan. It touches reproduce.rs plus its CLI args, schedule.rs, findings.md, review-apply.md:137-146, and possibly apply-v2-engine step 5 (route A).

- **Library/API or file**: tomlctl/src/plan/view.rs:740-741 `spec_block`
- **Source**: file read; tests/plan_view.rs:778-800; cli/types/mod.rs:182
- **Evidence-grade**: high
- **Finding**: the view already separates worker checks from gate checks through `writes_build`, but labels the split `Writes build`, which is false for `reproduce-status`. Relabelling it as run-by-gate is the smallest fix for B-9728b95d.
- **Details**: agents' line 52 already says a `yes` check is the gate's. Task 52's worker ran it anyway. Renaming the label moves the plan_view tests, plan.md:341-342, README.md:569, both agents, and the `plan_view_writes_build` capability token the pre-flights check.
- **Counter**: if workers ignore an explicit "Run by: gate" label as they did "Writes build: yes", the relabel fixes nothing. The worker prose would need a hard rule instead. That is unverifiable without a run.
- **Impact on plan / report**: small, but the capability token and pre-flight must move together.

Coverage: read in full tomlctl/src/review/schedule.rs. Read in part: fields.rs 600-822, 1388-1440 and 2070-2083; store.rs 406-419; reproduce.rs 1-40 and 230-280; select.rs 165-181; tasks/check.rs 375-420 and 590-655; tasks/graph.rs 340-360; plan/view.rs 705-760; tasks/schema.rs 270-290; drain.md 185-260; findings.md 100-175. Grepped apply-v2-engine SKILL.md and plan.md:330-345, and swept `Writes build` and `edited_paths` over the tree minus target and docs/flows. Ruled out: changing `tasks check` to skip done pairs (out of lens, schedule route asked), and parsing `fix` prose for paths as the primary source (heuristic; fallback only). Ran no build or test.
