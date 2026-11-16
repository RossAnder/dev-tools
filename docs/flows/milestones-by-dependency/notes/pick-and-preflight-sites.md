Verdict: The store-order assumptions sit in engine prose: the pick, the `--milestone` refusal, the deferred stop and continue-to-next (run-milestone.md:58-73, SKILL.md:38, drain.md:476). The flow-wide ones are three tomlctl verbs with no milestone scope: `plan stage` open runs (stage.rs:60-101), `plan check` (check.rs:56-100) and `tasks check`. `checkpoint/invalid-cut` (tasks/check.rs:665) and `need-not-done` (emit.rs:748) can stay as they are if checkpoint store order stays a topological order of the milestones.

## Answer

- **The pick and its three companions are prose. tomlctl has no notion of the "next milestone".** The engine finds the milestone by walking `[[checkpoints]]` in store order for the first one with a row that is not `done` (run-milestone.md:58-65). It refuses `--milestone` while any *earlier* checkpoint is unfinished (SKILL.md:38, run-milestone.md:58). It stops on a deferred milestone (run-milestone.md:67-73). It goes on only while "nothing in the flow is parked" (drain.md:476, SKILL.md:64). For a dependency-ordered pick, each rule should test a milestone's *upstream* milestones, not the milestones before it in the store:
  - **The pick.** Take the first checkpoint, in store order, that meets all of these:
    - it has a row that is not `done`;
    - it is not deferred;
    - every milestone holding a member of its `tasks closure --checkpoint <id>` `dependency_closure` outside its own members has all its rows `done` and stamped.
  - **A deferred milestone** is skipped and reported, not a stop.
  - **The `--milestone` refusal** tests upstream milestones, not earlier ones.
  - **Continue-to-next** stops only when no milestone is runnable. A parked row then holds back only its own milestone's gate and the milestones downstream of it.
  - **A new verb.** Done in prose, this is one `closure` call plus one `list` call per checkpoint. A small read verb that prints `{runnable, blocked:{id, on}, deferred}` per checkpoint keeps the rule in one place. glimpse could show the same result.
- **If store order stays a topological order, tasks check needs no change.** `checkpoint/invalid-cut` is a prefix-closure test in store order (graph.rs:364-396, tasks/check.rs:665-702). v1 `/implement` commits against it (implement.md:175), so it cannot be relaxed. It stays sound because it already forces every upstream milestone to come earlier in the store. "First runnable in store order" is then a valid dependency pick. B-a505bedd's reorder becomes only a tie-break preference.
- **`need-not-done` is already decided per row, not by store order.** Only its documented meaning changes (run-milestone.md:171). Two gaps:
  - It reads `needs` only (emit.rs:754), while the graph's reachability is `needs ∪ coupling` (graph.rs:213). The pick must take upstream milestones over both, which `closure` already does.
  - It accepts `done` without a stamp, so the pick must require upstream milestones to be committed.
- **The pre-flight is flow-wide in five places:**
  - `plan stage` lists every open run, whatever its step, as a blocker (stage.rs:90-101). A planning run such as `cold-read` carries no milestone in its step name (write-and-prove.md:233). The Re-entry rule settles only `implement:` and `reproduce:` runs (SKILL.md:89).
  - `plan check` and `tasks check` take no `--milestone` (cli/types/plan.rs:498).
  - The stale `in-progress` reset covers every row and waits on any open run (run-milestone.md:47-54).
  - The clean-tree exemption covers every started row in the flow (run-milestone.md:37-43).

  To scope them, keep the graph-structural classes and the classes whose finding names no task flow-wide. Filter the per-row classes to the picked milestone and its upstream rows.
- **`plan/milestone-width`** runs before approval only (check.rs:94). Its "could join" target is the store index of the latest need plus one (check.rs:772). That target is still sound under topological order, but it can name a deferred milestone, so it should skip those. A dependency pick also removes much of the wait it measures.
- **glimpse** is unaffected while runs stay serial. `running_milestone` is the first open `implement:` run (plan/mod.rs:43-49), and `waits_on_milestone` assumes there is one (glimpse/src/model.rs:544-555). Checkpoints are drawn by layer, not by store order (glimpse/src/view/layers.rs:73-84).

## Findings

- **Library/API or file**: claude/skills/implement-v2-engine/references/run-milestone.md:58 (pick), :67-73 (deferred stop); claude/skills/implement-v2-engine/SKILL.md:38 (`--milestone` refusal), :64; claude/skills/implement-v2-engine/references/drain.md:476 (Next), :521 (report's next line)
- **Source**: file reads at 5aca3e7b
- **Evidence-grade**: high
- **Finding**: The pick, the refusal, the deferred stop and continue-to-next all test store position or the whole flow. Each must instead test the milestone's upstream closure over `needs ∪ coupling`, with upstream milestones `done` and stamped (needs-plan).
- **Details**: The walk is `items list --array checkpoints` followed by one `tasks list --where checkpoint=<id> --where-not status=done` per checkpoint (run-milestone.md:61-62). `tasks closure --checkpoint <id>` already prints `dependency_closure` (tasks/closure.rs:50-91), so upstream milestones can be derived without new graph code. The runnable/blocked/deferred answer is better made one read verb than a walk in prose. With a parked row the milestone still "runs its disjoint work and cannot gate" (SKILL.md:113). Under a dependency pick, the engine should go on to milestones not downstream of the parked row.
- **Counter**: The finding is wrong if some rule outside these sites makes later milestones wait on earlier ones. I swept `store order|first checkpoint|next milestone` across `claude/` and `tomlctl/src/plan`. The only other hit is apply-rows.md:215 (next record).
- **Impact on plan / report**: These are the core edits: three engine docs, plus an optional read verb that the pick and glimpse would share.

- **Library/API or file**: claude/skills/implement-v2-engine/references/apply-rows.md:215; claude/skills/apply-v2-engine/SKILL.md:167-174; tomlctl/src/plan/milestone.rs:143 (`milestone add` appends)
- **Source**: file reads
- **Evidence-grade**: high
- **Finding**: The settle drain schedules accepted fixes into "the checkpoint that follows this one in store order". Nothing in the task graph links `settle-<n>` to `apply-<n>`, so a dependency pick can run and commit `apply-<n>` before the settle drain adds rows to it.
- **Details**: `apply-v2-engine` adds `settle-<n>` and then `apply-<n>`, and `apply-<n>` may start empty (SKILL.md:174). A milestone with no row that is not `done` is never picked, so that case is safe. A non-empty `apply-<n>` whose rows are independent of the settle rows would run first and then be reopened by `review schedule`. Name the target by its id pair, not by adjacency. Then decide whether to accept a reopened milestone (a second commit pair) or to add a milestone-level ordering edge, which the store does not have.
- **Counter**: The finding fails if `review schedule` or the settle rows make the fix rows need the settle rows. Nothing in apply-rows.md:215-222 says they do. I did not read `review schedule`'s source.
- **Impact on plan / report**: One extra engine site, plus a decision on reopen versus an ordering edge.

- **Library/API or file**: tomlctl/src/tasks/check.rs:665 `cut_findings`; tomlctl/src/tasks/graph.rs:364 `Graph::groups`; claude/commands/implement.md:175
- **Source**: file reads; test `a_group_depending_on_a_later_group_is_an_invalid_cut` (tasks/check.rs:798)
- **Evidence-grade**: high
- **Finding**: Keep `checkpoint/invalid-cut` unchanged. It is the invariant that makes "first runnable checkpoint in store order" a correct dependency-ordered pick.
- **Details**: `valid_cut` requires each store-order prefix to be downward-closed. The upstream milestones of any checkpoint therefore sit earlier in the store. v1 `/implement` refuses to commit on a false `valid_cut`, so replacing the rule with a quotient-graph cycle check would break v1. The same groups also feed `render` (tasks/render.rs:260) and the glimpse snapshot (tasks/snapshot.rs:124).
- **Counter**: The finding fails if the plan lets milestones be stored in non-topological order, for example narrative order with a downstream milestone first. That needs a v2-only cycle class in `plan check`, with `invalid-cut` demoted for v2 flows.
- **Impact on plan / report**: No `tasks check` change is needed. Write-and-prove gains only a tie-break rule: B-a505bedd's "independent upfront milestones before a deferred one".

- **Library/API or file**: tomlctl/src/plan/emit.rs:748 `stranded`; tomlctl/src/tasks/graph.rs:213; claude/skills/implement-v2-engine/references/run-milestone.md:171
- **Source**: file reads
- **Evidence-grade**: high
- **Finding**: `need-not-done` checks only `needs`, and accepts a `done` need whether or not it is stamped. A dependency pick must compute upstream milestones over `needs ∪ coupling` and require them to be committed. Only the row's prose in run-milestone.md:171 needs editing.
- **Details**: `stranded` loops `candidate.row.needs` and counts `Status::Done` or dispatched as fine. `coupling` goes into `in-degree` and `closure_up`, but this check never reads it. Under store-order pick plus a valid cut, a coupling target was always in a finished earlier milestone, so the gap could not bite.
- **Counter**: The finding fails if `drop_dependents` or another step in `plan emit launch-args` already catches an undone coupling target. `drop_dependents` walks only the dependents of rows already dropped (emit.rs:769-771).
- **Impact on plan / report**: The pick verb or prose takes over the cross-milestone guard. Optionally, `stranded` could also read `coupling`.

- **Library/API or file**: tomlctl/src/plan/stage.rs:60-101 (`with_open_runs`), :169-193 (post-approval blockers); tomlctl/src/plan/check.rs:56-100 `findings`; tomlctl/src/cli/types/plan.rs:498 `PlanCheckArgs`; claude/skills/implement-v2-engine/references/run-milestone.md:17-26, :37-54; claude/skills/implement-v2-engine/SKILL.md:89-109
- **Source**: file reads
- **Evidence-grade**: high
- **Finding**: Each pre-flight check covers the whole flow. A milestone-scoped pre-flight needs four changes (needs-plan):
  - a `--milestone` filter on `plan check`, or on `tasks check` as well;
  - an open-run rule for planning steps;
  - a stale-row reset scoped to the milestone;
  - a decision on the clean-tree exemption.
- **Details**:
  - **Open runs.** After approval, `plan stage` reports only the brief findings and every open run, whatever its step (`open run: step cold-read` was observed in B-c2d75b04). Run rows carry a milestone only in an `implement:<id>` or `reproduce:<id>` step name. To tell a planning run on another milestone from one on this milestone, `cold-read` and similar steps would need an `:<id>` suffix or a `tasks` list.
  - **Check scoping.** `plan check` merges `tasks check` with the v2 classes. Every finding carries `ids`, so per-row classes such as `plan/task-no-spec`, `spec-no-accept`, `probe-unhealthy`, `design-anchor-dead` and `premise-task-unknown` can be filtered to the picked milestone and its upstream rows. These classes must stay flow-wide:
    - the `dag/*` classes and `checkpoint/invalid-cut`, because `Graph::build` refuses the snapshot and the emit on them;
    - the brief classes;
    - findings that name no task, such as `plan/spec-orphan`.
  - **Clean-tree exemption.** It covers the files of every started row (run-milestone.md:37-43). The gate of a milestone picked out of order therefore runs on a tree that also holds a skipped milestone's partial edits.
- **Counter**: The finding fails if the flow is meant never to have two sessions active, so that nothing writes another milestone's rows mid-run. Even then, a parked or failed row's half-done spec state still stops `plan check` flow-wide.
- **Impact on plan / report**: These sites are B-c2d75b04's. Separately, the drain's lane check digests whole store sections (plan/lane.rs:1-13), so a concurrent elaboration write would read as a worker out-of-lane write. That check belongs to a sibling question, not this one.

- **Library/API or file**: tomlctl/src/plan/check.rs:729 `milestone_width_findings`
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: `plan/milestone-width` stays valid under a topological store order. It should skip deferred milestones as targets, and the guidance at write-and-prove.md:206 should say so.
- **Details**: The target is `max(index of a need's checkpoint) + 1` over store order, and only the source milestone is checked for deferral (check.rs:753-756, :772). The finding can therefore advise moving a row into a deferred milestone, which would hold that row back. It runs only while `plan.approvals` is empty (check.rs:94).
- **Counter**: The finding fails if a deferred milestone can never sit at index `latest + 1` for any row. No rule in write-and-prove.md:53-57 places deferred milestones.
- **Impact on plan / report**: A small change to `plan check`, or none if the plan accepts it as info noise.

- **Library/API or file**: glimpse/src/model.rs:544 `waits_on_milestone`; tomlctl/src/plan/mod.rs:43 `running_milestone`; tomlctl/src/tasks/snapshot.rs:123-124
- **Source**: file reads
- **Evidence-grade**: high
- **Finding**: glimpse's milestone view needs no change for a serial dependency pick. Two changes are optional:
  - showing per-milestone runnable, blocked or deferred status, from the read verb proposed above;
  - multi-valued `running_milestone`, needed only if two implement runs could be open at once.
- **Details**: `running_milestone` takes the first open `implement:` run. When no run is open, every ready row reads as dispatchable whatever its milestone (test `no_running_milestone_leaves_the_frontier_dispatchable`). Under a dependency pick that reading is closer to the truth than it is today.
- **Counter**: The finding fails if the plan permits concurrent `implement:` runs. `plan run start` does not refuse a second open run (plan/records.rs:618-675).
- **Impact on plan / report**: No change is needed unless concurrency is in scope.

Coverage: Read in full: implement-v2-engine SKILL.md, run-milestone.md and plan/stage.rs (to line 340). Read in part:
- drain.md steps 5, 6 and 10 to 11;
- apply-rows.md:205-229;
- apply-v2-engine SKILL.md:155-179;
- write-and-prove.md:44-59 and 200-211;
- plan/check.rs:1-200, 436-660 and 720-800;
- tasks/check.rs:640-815, graph.rs:150-175 and 355-396, closure.rs, emit.rs:690-780, snapshot.rs:110-156, plan/records.rs:600-690, plan/milestone.rs:120-160, lane.rs header;
- glimpse model.rs:430-590 and view/layers.rs:30-85.

Sweeps: `store order|first checkpoint|next milestone|downward|invalid-cut|valid_cut` over `claude/` and `tomlctl/src`, `--step` names in plan-v2-engine, and `running_milestone|waits_on_milestone` in glimpse. Ruled out:
- `tasks ready --checkpoint`, `plan probes --milestone` and `plan emit gate-list` are already milestone-scoped and gate across milestones only by edges.
- The `plan stage` classification (approved, running or done) is flow-wide by intent.
- `plan/milestone-no-gate` and `plan/deferred-no-reason` are per-milestone and do not depend on order.

```toml sources
read_at = "5aca3e7be8f169ba2b524edbb4e87b955e5b67da"
dirty = []

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 58
claim = "pick is the first checkpoint in store order with a row not done; --milestone refused while an earlier checkpoint is unfinished"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 67
claim = "a deferred milestone stops the engine"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 17
claim = "pre-flight stage check stops on any blocker other than an open run; plan check and tasks check run flow-wide"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 37
claim = "clean-tree exemption covers every started row's files flow-wide"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 171
claim = "need-not-done documented as a wrong milestone cut"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 38
claim = "--milestone refusal by earlier checkpoint in store order"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 89
claim = "open-run rule settles only implement and reproduce runs"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 476
claim = "continue to next milestone only when nothing in the flow is parked"

[[source]]
path = "claude/skills/implement-v2-engine/references/apply-rows.md"
line = 215
claim = "settle drain schedules into the checkpoint following this one in store order"

[[source]]
path = "claude/skills/apply-v2-engine/SKILL.md"
line = 167
claim = "settle-n and apply-n are paired only by being added in sequence; apply-n may start empty"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 53
claim = "How many rule and milestone placement; no rule on where deferred milestones sit"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 233
claim = "cold-read run step carries no milestone id"

[[source]]
path = "claude/commands/implement.md"
line = 175
claim = "v1 /implement relies on valid_cut prefix semantics, so invalid-cut cannot be relaxed"

[[source]]
path = "tomlctl/src/plan/stage.rs"
line = 90
claim = "every open run of any step becomes a stage blocker"

[[source]]
path = "tomlctl/src/plan/stage.rs"
line = 169
claim = "post-approval stage reports only brief findings plus open runs"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 56
claim = "plan check findings are flow-wide with no milestone filter"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 729
claim = "milestone-width targets store index of latest need plus one, deferred target not skipped, pre-approval only"

[[source]]
path = "tomlctl/src/cli/types/plan.rs"
line = 498
claim = "PlanCheckArgs has no --milestone"

[[source]]
path = "tomlctl/src/tasks/check.rs"
line = 665
claim = "checkpoint/invalid-cut is a store-order prefix closure test"

[[source]]
path = "tomlctl/src/tasks/graph.rs"
line = 364
claim = "Graph::groups computes valid_cut over store-order prefixes"

[[source]]
path = "tomlctl/src/tasks/graph.rs"
line = 213
claim = "graph reachability uses needs and coupling"

[[source]]
path = "tomlctl/src/tasks/closure.rs"
line = 50
claim = "tasks closure --checkpoint prints dependency_closure usable to derive upstream milestones"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 748
claim = "need-not-done reads needs only and accepts done without a commit stamp"

[[source]]
path = "tomlctl/src/plan/milestone.rs"
line = 143
claim = "milestone add appends the checkpoint at the end of store order"

[[source]]
path = "tomlctl/src/plan/records.rs"
line = 618
claim = "plan run start does not refuse a second open run"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 1
claim = "lane check digests whole store sections"

[[source]]
path = "tomlctl/src/plan/mod.rs"
line = 43
claim = "running_milestone is the first open implement run"

[[source]]
path = "tomlctl/src/tasks/snapshot.rs"
line = 123
claim = "snapshot checkpoint groups come from store order"

[[source]]
path = "glimpse/src/model.rs"
line = 544
claim = "waits_on_milestone assumes a single running milestone"

[[source]]
path = "glimpse/src/view/layers.rs"
line = 73
claim = "glimpse places checkpoints by their members' layer, not store order"
```
