Verdict: a worker could carry out milestone A as written apart from 3 points: 2 unactionable (tasks 15 and 22) and 1 false premise (task 19); the other 19 tasks are actionable.

```toml sources
read_at = "00ff2113dbfab0b7db0b98df71442194340b81c6"
dirty = ["docs/flows/review-v2-cells-and-handoff/design/direction.md"]

[[source]]
path = "claude/skills/review-v2-engine/references/agent-waves.md"
line = 155
claim = "agent-waves.md has no passage about a verifier dispatch after a convert (grep -ci convert prints 0); the nearest is Mop-up's single-finding verifier"

[[source]]
path = "claude/skills/review-v2-engine/references/agent-waves.md"
line = 136
claim = "agent-waves.md already contains 'escalation', so task 19's agent-waves clause passes on the present tree"

[[source]]
path = "claude/skills/review-v2-engine/references/triage-and-close.md"
line = 140
claim = "Convert points to a one-row dispatch 'per agent-waves.md', which that file does not describe for a convert"

[[source]]
path = "tomlctl/src/review/stats.rs"
line = 819
claim = "apply_state exists, but no notion of a cluster root is defined in stats"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 1417
claim = "route() takes no fix_files today; task 12 adds it, and task 22 documents it without needing task 12"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 550
claim = "FIX_FILES is [R, R, N, ...], so task 3's premise holds"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1064
claim = "propose's planned count filters agent != EXECUTOR, which task 1 lifts into planned_cells"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 2123
claim = "the routing parser accepts only executable and tests (task 4 premise holds)"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 76
claim = "SliceRule has four variants; slices_for matches them with wildcards, so new variants compile"

[[source]]
path = "tomlctl/src/review/schedule.rs"
line = 581
claim = "schedule's needs come only from depends_on (task 13 premise holds)"

[[source]]
path = "tomlctl/src/review/digest.rs"
line = 948
claim = "coverage() ends with the Waivers section, where task 11 adds its sections"

[[source]]
path = "tomlctl/tests/review_cells.rs"
line = 13
claim = "SCOPE, VOCABULARY (line 15) and STORE (line 434) are used by the helpers task 2 moves but are not in its move list"

[[source]]
path = "claude/workflows/review-v2.js"
line = 367
claim = "escalation text is cut to ESCALATION_CHARS in the Workflow index, and no task in this milestone edits the Workflow"

[[source]]
path = "docs/flows/review-v2-cells-and-handoff/design/direction.md"
line = 33
claim = "the direction names `review escalate` and `review triage <cell>:<n> --fix`, while tasks 9, 10, 18 and 19 and D12 specify `review escalation record` and `review escalation fix`"
```

## Unactionable

1. **Task 15, unactionable.** Detail says "applied counts unique clusters whose root's apply_state is scheduled or resolved" and "A same_as to a finding of an earlier run is counted apart as `cross_run`". The task never says which member of a same_as/merged_into cluster is its root: the merged_into target, the same_as target, or the oldest id. One of the task's own tests has a first-pass row merged into a repeat row, so the choice changes the count. The `cross_run` value in `runs: [{..., cross_run}]` has no shape either: one count per run, one count per arm, or a list of finding ids. A worker would have to guess both. The existing `apply_state` helper (stats.rs:819) works on one finding and gives no help with clusters.
2. **Task 22, unactionable (missing dependency).** Its Detail bullet "Routing: a cleanup's fix_files count toward its route" documents what task 12 builds (route() gains fix_files, fields.rs:1417). Task 22's Needs are 3, 7, 8, 10, 14 and 15, not 12. Task 12 needs only 4, so it can still be running when task 22 starts, and task 22 says "Read the shipped code for every flag and key spelling". A worker starting early would document behaviour that is not in the code yet. Adding 12 to task 22's Needs closes this.

## False premises

1. **Task 19.** Claim: "agent-waves.md: the one-row verifier dispatch after a convert also covers a finding minted by an escalation fix." No such passage exists. `grep -ci convert claude/skills/review-v2-engine/references/agent-waves.md` prints 0. The only single-finding verifier rule is under Mop-up (agent-waves.md:155-169), which is about unverified ids from a lost return. triage-and-close.md:140 does say "verify it with a one-row dispatch per `agent-waves.md`", but agent-waves.md never describes that dispatch for a convert. The worker has to decide whether to extend Mop-up or write a new passage. That task's agent-waves check clause `grep -q 'escalation' agent-waves.md` cannot catch either choice, because it already passes on the present tree (agent-waves.md:136, 147).

## Observations (not counted)

- **Task 2:** the move list leaves out the constants `SCOPE`, `VOCABULARY` and `STORE` (review_cells.rs:13, 15, 434). `write_catalogue`, `Repo::new`, `Repo::store` and `Repo::plan_cmd` use them, so the move does not compile without them. A worker will move them as well. Naming them would save a guess.
- **Direction vs tasks:** direction.md#milestone-a, Escalations, names `review escalate` and `review triage <cell>:<n> --fix`. D12 and tasks 9, 10, 18 and 19 specify `review escalation record` and `review escalation fix <cell>:<n>`. Each task's Detail settles which to build, but the direction is stale on the verb surface.
- **D4** binds `claude/workflows/review-v2.js` and `tomlctl/src/review/runs.rs`, and no task in A lists either file. The Workflow still cuts escalation text to 300 chars (review-v2.js:367), so `review escalation record` will store text that is already truncated. This is about plan coverage, not a worker blocker.
- **Task 8:** existing review_cells tests that append waves pad only up to the ceiling (for example review_cells.rs:609, padding to 3). I found no existing test outside its files that would hit the new refusal. I did not trace every test file that appends cells.
- **Task 16:** the guard `! grep -qi ... u5/tracked.txt u5/run.toml` also passes when the files are missing, because grep's exit 2 is negated. The existence check beside it covers this.

## Check commands (run on the present tree, HEAD 00ff2113)

All are the tasks' own read-only forward checks. Each failed (exit 1 or 2) as expected before its change, unless marked otherwise:

- T1 `test ! -f .../cells.rs && test -f .../cells/mod.rs ...` exit 1; `grep -q 'fn planned_cells' .../cells/mod.rs` exit 2 (no file). Premise `grep fn propose( / fn append( cells.rs` exit 0 (holds).
- T2 `grep -q '^mod review_harness;' tomlctl/tests/it.rs` exit 1.
- T3 field-spec check exit 1; premise `FIX_FILES ... [R, R, N,` exit 0 (holds).
- T4 `grep signal-evidence catalogue.rs` exit 1, `grep lockfiles catalogue.rs` exit 1; premise `f.only(&["executable", "tests"]` exit 0 (holds).
- T5 `grep '^slice = "signal-evidence"' security.md` exit 1; `grep changed-with-tests SKILL.md` exit 1.
- T6, T7, T8, T10 checks exit 2 (target files do not exist yet).
- T9 `test -f escalation.rs` exit 1. T11 `grep 'Ceiling overrides' digest.rs` exit 1. T12 `grep -A12 'pub(crate) fn route(' | grep fix_files` exit 1.
- T13 `grep '"ordered"' schedule.rs` exit 1, `grep ordered review-apply.md` exit 1; premise `for dep in &f.depends_on` exit 0 (holds).
- T14 `grep audit_classes stats.rs` exit 1. T15 `grep 'long = "yield"' stats.rs` exit 1.
- T16 `test -s .../u3/tracked.txt` exit 1; the guard exits 0 (vacuous, see above). Premises: the U5 store and `d1930996` are present, and the U3 run and `c7c85ae9` exist, so both hold. U3 and U5 are both `standard`, so `--context-free` is allowed.
- T17 `grep '"review_propose_kinds"' cli/types/mod.rs` exit 1.
- T18 `grep over-ceiling-file compose.md` exit 1, `grep review_plan_ceiling SKILL.md` exit 1.
- T19 `grep 'review escalation fix' triage-and-close.md` exit 1, `grep fix_files triage-and-close.md` exit 1; `grep escalation agent-waves.md` exit 0 (cannot discriminate).
- T20 `grep fix_files review-checker.md` exit 1; the verifier Cleanup clause exit 1.
- T21 `grep -- --yield measurement.md` exit 1, `grep over-ceiling-file plan-review-lead.md` exit 1.
- T22 `grep 'review escalation' review.md` exit 1, `grep ceiling_overrides review.md` exit 1.
- Contracts C1 exit 2, C2 exit 1, C3 exit 1, C4 exit 1.
- I did not run the nextest or `cli::dispatch::tests` checks. They build the crate and belong to the gate.

## Coverage

I read the worker views and file lists for tasks 1 to 22. Against the code I checked: cells.rs (every item in task 1's boundary list exists, and the cross-module imports in cover.rs, digest.rs, runs.rs, stats.rs and mod.rs match the re-export list), review_cells.rs helpers, it.rs, fields.rs (FIX_FILES, Role order, route, RoutingGlobs and its callers, the finding field names), store.rs Cell, catalogue.rs (SliceRule, Routing, the routing parser), digest.rs coverage(), schedule.rs Compose::plan and row, stats.rs (Tally, by_model, by_run, apply_state, flags), triage.rs (replacement_row, finish_row), mod.rs ReviewOp, the five lens frontmatters, the lenses SKILL.md, compose.md, triage-and-close.md, agent-waves.md, review-checker.md, review-verifier.md, plan-review-lead.md, capabilities FEATURES and its test, and the U3 and U5 sources. Not checked: whether every existing test that appends cells stays under the new ceiling, the gate-only test runs, and measurement.md's and review.md's section layout in detail.
