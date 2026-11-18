Verdict: `review schedule` links fix tasks only by exact edited-path overlap (`fold`, tomlctl/src/review/schedule.rs:471) and by finding `depends_on` (schedule.rs:581), which no finding in eight stores has ever carried; the reliable signal of an interaction is a prose-target finding whose `edit`/`old`/`fix`/`fix_sketch`/`summary` text names another task's executable edited path, and that should become a `needs` edge from the doc task to the code task, not a merge.

```toml sources
read_at = "00ff2113dbfab0b7db0b98df71442194340b81c6"
dirty = []

[[source]]
path = "tomlctl/src/review/schedule.rs"
line = 471
claim = "fold groups chosen findings only where their edited paths are equal strings, transitively"

[[source]]
path = "tomlctl/src/review/schedule.rs"
line = 449
claim = "edited() = target_file or file, instances, fix_files; no prose field is read"

[[source]]
path = "tomlctl/src/review/schedule.rs"
line = 578
claim = "needs comes only from each finding's depends_on, mapped through task_of, unmapped ids go to dropped_deps"

[[source]]
path = "tomlctl/src/review/schedule.rs"
line = 686
claim = "row(): detail, constraints, premises at base, executor rule; nothing about sibling tasks"

[[source]]
path = "tomlctl/src/review/schedule.rs"
line = 803
claim = "accept is anchor --gone for remove cleanups, else the milestone-wide reproduce-status gate check"

[[source]]
path = "tomlctl/src/review/store.rs"
line = 407
claim = "Finding::edited_paths reads file/target_file and instances"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 644
claim = "defect carries fix (<=800); cleanup carries edit (<=800), direction, old, target_file, instances; fix_files on both"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 803
claim = "depends_on is a finder-writable StrList with no validation or computed writer"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 1363
claim = "RoutingGlobs (vocabulary [routing] executable and tests globs) already classify a path as executable code"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 518
claim = "RoutingGlobs is built from the catalogue vocabulary"

[[source]]
path = "claude/skills/tomlctl/references/review.md"
line = 119
claim = "depends_on is an optional field on review add; no agent guidance says when to set it"

[[source]]
path = "claude/skills/tomlctl/references/review-apply.md"
line = 24
claim = "documented grouping and needs rule for review schedule"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 137
claim = "an after:<m> premise inside the milestone releases the task once m is done and the worker re-runs the check before editing"

[[source]]
path = ".claude/flows/mid-run-requests/review-v2.toml"
claim = "F48 (request.rs add refusal) and F62 (generated.md edit naming request.rs add) scheduled as tasks 23 and 30 with no edge"
```

## Answer

How it works today. `selection` (schedule.rs:341) picks chosen findings; `fold` (schedule.rs:471) unions them into one task wherever `edited()` (schedule.rs:449: `target_file` else `file`, `instances`, `fix_files`) share an identical path string; `Compose::plan` (schedule.rs:536) mints one row per group and builds `needs` solely from `f.depends_on` (schedule.rs:578-597). No prose field, symbol or lens is consulted, and all rows share one `--checkpoint`, so file-disjoint tasks run concurrently in the pool.

Why the backlog case slipped. F48 (defect, `tomlctl/src/plan/request.rs::add`) and F62 (cleanup, `claude/skills/tomlctl/references/generated.md`) share no path, so they were tasks 23 and 30 with no edge. F62's `edit` literally names `tomlctl/src/plan/request.rs add`, and its verifier citation is `request.rs:62`. That is the detectable signal. `depends_on` cannot carry it: the two came from different cells (C28, C23) written in parallel, a finder never sees the other cell's ids, and triage has no flag that writes `depends_on`. A count over all eight stores shows 0 of 512 findings with `depends_on`.

Which fields detect it, measured. I ran a read-only script over every review-v2 store, looking at pairs in one run whose edited paths are disjoint:
- Taking A's `edit`, `old`, `fix`, `fix_sketch`, `summary` and `failure_scenario` that contain B's `file` path gives 7 pairs out of 512 findings (76 scheduled).
- Restricting A to a non-code edit target leaves 6 pairs, which reduce to 4 task edges (mid-run 30←23, 31←23; ui-fidelity 45←42, 45←51).
- Also requiring B to be a `defect` leaves only 30←23 (the real conflict) and 31←23 (F73 doc ↔ F67 holders.rs, same subsystem, harmless to order).
- Adding `evidence` or the verdict `citation` raises it to 41 pairs, many of them incidental mentions in v2-apply. Leave those two out.
- `symbol` on its own is too generic (`add`, `run`, `text` match unrelated prose). Use it only as a tiebreak, never as the trigger.

Shape of the fix. Put a `needs` edge from the prose-target task to the code task, not a merge. A false positive then costs only serialisation inside the milestone. A merge would inflate the task and flip the executor to `deep` (schedule.rs:762). The edge alone does not make the doc worker re-judge, because F62's `edit` prescribes the old behaviour's wording and `review anchor` checks only that the `old` text is present. So the row's `detail` (or a constraint) should name the upstream task and its file::symbol and tell the worker to check the edit against that code once it has landed, returning `failed` if the finding no longer holds. An `after:<n>` premise (run-milestone.md:137) is the existing mechanism for a check timed after another task.

## Findings

- **Library/API or file**: tomlctl/src/review/schedule.rs:578 `Compose::plan` (needs from `depends_on` only)
- **Source**: schedule.rs:578-597; fields.rs:803; a count over `.claude/flows/*/review-v2.toml` printed 0 findings with `depends_on` in every store (512 findings in total)
- **Evidence-grade**: high
- **Finding**: The only edge source schedule has is a field no producer writes, so every cross-cell fix interaction mints independent parallel tasks. needs-plan
- **Details**: `depends_on` is optional on `review add` (review.md:119), has no guidance in review-v2-method or the finder agents, and cannot name another cell's ids because cells run in parallel. `triage` has no flag that sets it. Schedule is the first point where all chosen findings and their prose are in hand.
- **Counter**: A producer (finder, verifier or triage) that writes `depends_on` across cells would make schedule's existing mapping sufficient. A grep of tomlctl/src/review and claude/skills for a writer found none.
- **Impact on plan / report**: Detection belongs in `review schedule` (or a triage-time link verb), not in finder guidance.

- **Library/API or file**: tomlctl/src/review/schedule.rs:471 `fold`, :449 `edited`
- **Source**: `tomlctl review show F48,F62 --slug mid-run-requests --for implementer`. F62.edit says "Checked against tomlctl/src/plan/request.rs add". F48.file = tomlctl/src/plan/request.rs, symbol add. Task ids 23 and 30.
- **Evidence-grade**: high
- **Finding**: A prose-target finding whose `edit`/`old`/`fix`/`fix_sketch`/`summary`/`failure_scenario` contains the repo-relative path of another chosen finding's executable edited path should give its task `needs` on that finding's task.
- **Details**: Classify "executable" with the vocabulary's `[routing]` executable globs (`RoutingGlobs`, fields.rs:1363, built in catalogue.rs:518), which schedule does not load yet. Match B's path as a substring so that `path:line` counts. Collapse edges per task. Skip a pair already folded together, and skip an edge that would close a cycle: a folded group can hold both doc and code paths, so two groups can point at each other. Report a skipped edge as `dropped_deps` already reports one. A finding that names only a basename (for example "request.rs") is missed. That is a known recall limit.
- **Counter**: The measurement saw only 8 stores. A store whose doc edits routinely cite code paths for provenance would raise the edge count, and with it the lost parallelism. Rerun the scratch count on a new store to check.
- **Impact on plan / report**: Decides the detection fields (edit, old, fix, fix_sketch, summary, failure_scenario; not evidence, not citation, not symbol alone, not lens) and the edge direction (doc→code).

- **Library/API or file**: tomlctl/src/review/schedule.rs:686 `Compose::row`, :803 `accept`
- **Source**: schedule.rs:746-755 (premises `review anchor <F>` at base); `review anchor` holds while the `old` text is present
- **Evidence-grade**: medium
- **Finding**: Ordering alone does not stop F62's contradiction. The doc task's prescriptive `edit` still writes the pre-fix wording, and its base anchor premise still holds after the code task lands.
- **Details**: The minted row needs a line in `detail` or `constraints` naming the upstream task and its file::symbol, telling the worker to check the edit against that code and return `failed` if the finding no longer holds. It could also carry an `after:<n>` premise, which the pool already honours inside a milestone (run-milestone.md:137). I did not confirm that `review anchor` is a sound after-premise check for this purpose: it tests text, not behaviour.
- **Counter**: If the plan-worker procedure already re-verifies a finding's claim against current code before editing, the detail line is redundant. I did not read the worker procedure for this.
- **Impact on plan / report**: The schedule change carries both the `needs` edge and a worker-facing instruction. Test with a fixture pair shaped like F48/F62.

Coverage: read in full tomlctl/src/review/schedule.rs, plus store.rs:160-440 and fields.rs:600-810, 1355-1440, 1730-1810. Read review-apply.md `schedule`, run-milestone.md premise rules, and backlog item B-62be032f. Ran `review show` on F48, F62, F72, F73, F67 (mid-run-requests) and F47, F57, F59 (v2-ui-fidelity). Ran the scratch pair count over all 8 flow stores, with and without `evidence`. Ruled out: `lens` as a signal (doc-drift appears on both sides and says nothing about which file); `symbol` as a trigger (too generic); verdict `citation` and `evidence` (they tripled the pair count with incidental mentions); merging over ordering (it inflates tasks and flips the executor). Out of lens: the ordering of chosen findings and the triage-side link verb, which are a sibling question at most.
