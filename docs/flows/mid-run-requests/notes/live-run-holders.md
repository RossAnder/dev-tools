Verdict: The stores can tell which rows of a live run hold a path: the open `implement:<id>` run's `tasks` (tomlctl/src/plan/records.rs:674), filtered to rows still `pending` or `in-progress`, with each row's `files` frozen at launch (tomlctl/src/plan/emit.rs:656). But a widening granted under the any-row-of-the-milestone rule can still put two concurrent workers on one file in two ways: a second grant of the same path, since grants never reach `files` while the run is live, and a case-variant spelling the Workflow treats as one file (claude/workflows/implement-v2-milestone.js:157) but tomlctl compares case-sensitively (tomlctl/src/tasks/schema.rs:714).

```toml sources
read_at = "51fd3b2b88fa092fea7bfc510421e2a6c78f72b7"
dirty = ["docs/flows/mid-run-requests/brief.md"]

[[source]]
path = "tomlctl/src/plan/records.rs"
line = 625
claim = "plan run start records the deduplicated task ids, args_hash, store_digest and digest_at on a Run, and no files"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 548
claim = "Launch::build dispatches only pending milestone members and copies each row's files verbatim into args"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 139
claim = "args_sha256 hashes the whole args object, files included; only the hash reaches the store"

[[source]]
path = "claude/workflows/implement-v2-milestone.js"
line = 157
claim = "fileKey compares paths slash-normalised and case-folded"

[[source]]
path = "claude/workflows/implement-v2-milestone.js"
line = 269
claim = "canStart reads t.files from args at start time only; held is only the in-flight tasks' file keys"

[[source]]
path = "claude/workflows/implement-v2-milestone.js"
line = 276
claim = "siblings lists files of pending and in-flight tasks at each start, goes into the prompt only, and is never recorded"

[[source]]
path = "claude/workflows/implement-v2-milestone.js"
line = 243
claim = "settleBlocked removes a task behind a non-done result from pending without dispatching it; its row stays pending in the store"

[[source]]
path = "claude/agents/plan-worker-lite.md"
line = 31
claim = "a worker sets its own row in-progress with its agent name at step 6, after reading and an optional after-premise check"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 246
claim = "while a run is live the orchestrator writes no tasks field on the milestone's rows"

[[source]]
path = "docs/flows/mid-run-requests/brief.md"
line = 11
claim = "non-goal: no write to tasks.toml while a milestone run is live"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 714
claim = "normal_path folds backslashes and a leading ./ but not case"

[[source]]
path = "tomlctl/src/query.rs"
line = 1278
claim = "--where-contains is a case-sensitive substring test per array element"

[[source]]
path = "tomlctl/src/tasks/graph.rs"
line = 422
claim = "tasks ready's held/holder compares raw file strings, with no slash or case folding"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 423
claim = "the lane check's lanes are dispatched rows and started uncommitted rows, compared through normal_path"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 186
claim = "store_digest covers dispatched rows with status and agent blanked, so their files are in the digest"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 100
claim = "the open-run rule: never launch over a live run; another session's open run is abandoned only on the user's word"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 207
claim = "the drain's holder definition: pending, or agent set with empty commit"
```

## Answer

**Who holds a path, from the stores.** Take the live run as the newest `[[runs]]` row in `plan.toml` with step `implement:<id>` and no `finished`. Its `tasks` are exactly the ids of `args.tasks`. Every one of those rows was `pending` at the emit, and `files` comes straight from the row. No store records the args' `files` or the Workflow's `held` and `siblings` sets: the args go to a scratch file, the store keeps only `args_hash`, and `siblings` lives only in each prompt. They don't need recording, though. The orchestrator writes no `tasks` field during a live run, so each dispatched row's `files` in `tasks.toml` still equals the files the Workflow holds for it. The run's stored `store_digest` checks that: it covers those rows with `status` and `agent` blanked, so the `rows` section's digest still matching proves no file list moved.

A row of the run holds its files for the rest of the run, pending or in flight, exactly when its status is `pending` or `in-progress`:

- `in-progress` means in flight, or a dead worker (`no-return`, `error`) that the drain resets later. Either way it is treated as a holder.
- `pending` means one of three things: not started yet; started by the Workflow but before the worker's step 6 marked it, a window of reads plus an optional after-premise probe; or blocked behind a failed or parked gate and never dispatched. The store cannot separate these three, and it does not need to, since all of them count as holders. A blocked row is a false holder, which only costs a denial.
- `done`, `failed` and `parked` rows have returned and the Workflow released their files. A `done` row's edits still sit uncommitted until the gate.

**Can an any-row-of-the-milestone grant let two concurrent workers edit one file?** Inside the Workflow, no. The run's rows are a subset of the milestone's rows, their `files` are frozen, and `canStart` reads only those. So a path that no row of the milestone holds can never enter `held` for another task of this run. The rule is safe only if the holder set and the comparison meet two conditions. Each one, if broken, is a real path to two concurrent workers on one file:

1. **Earlier grants of the same run must count as holders.** A grant cannot be written to `files` while the run is live (brief non-goal; run-milestone.md:246). The any-row test reads `files`, so a second worker asking for the same path finds no holder and is granted too. Both workers are in flight on one file. The deny test therefore has to read the run's granted-but-unwidened paths from wherever the request records live (the `request-record` lens), keyed to this run.
2. **The comparison must be at least as coarse as the Workflow's `fileKey`, which is slash-normalised and case-folded.** Every tomlctl matcher available today is finer: `normal_path` (lane check) is case-sensitive, `--where-contains` is a case-sensitive substring test, and `tasks ready`'s `first_claim` compares raw strings. A row holding `src/Foo.ts` and a request for `src/foo.ts` would read as "no holder". On Windows or macOS that is one file, and that row may be in flight. The `--where-contains` substring test does err the safe way on `a.rs` versus `data.rs`, but that does not cover case.

One case lies outside the stores. A second session running another milestone at the same time is not seen by an any-row-of-*this*-milestone test. The open-run rule (SKILL.md:100-101) lets that happen only on the user's word, so it is a user override, not a gap a rule can close from the stores. A grant of a path held by a `done`, uncommitted row of the milestone is sequential, not concurrent. The any-row rule denies it anyway.

## Findings

- **Library/API or file**: tomlctl/src/plan/records.rs:625 `run_start`; tomlctl/src/plan/emit.rs:548 `Launch::build`
- **Source**: code reading at the anchors; run-milestone.md:239,246
- **Evidence-grade**: high
- **Finding**: The live run's holder set can be derived from the stores as {rows in the open `implement:<id>` run's `tasks`} ∩ {status `pending` or `in-progress`}, each with its current `files`, and needs no new record of the Workflow's `held` set.
- **Details**: `Run.tasks` holds the dispatched ids, deduplicated. `Dispatched.files = row.files.clone()` for pending members only. The orchestrator is barred from writing `tasks` fields while the run is live, so `files` matches the launch. A `rows` digest that still matches (`lane::store_digest` over `run.tasks`, status and agent blanked) proves it.
- **Counter**: The finding fails if anything writes a dispatched row's `files` while the run is live. The mid-run-requests design forbids that (brief non-goal), and a worker doing it shows up in the lane check's digest.
- **Impact on plan / report**: A run-time grant verb can compute holders from `plan.toml` and `tasks.toml` alone. A cheap verb or flag on the existing read paths is enough, and the Workflow needs no change, which matches the non-goal of "files read only at task start".

- **Library/API or file**: claude/workflows/implement-v2-milestone.js:269 `canStart` / :274 `start`; brief non-goal at docs/flows/mid-run-requests/brief.md:11
- **Source**: code reading; drain.md:207 holder definition
- **Evidence-grade**: high
- **Finding**: needs-plan. If the any-row-of-the-milestone test reads only `tasks.toml` `files`, it grants the same unheld path to two workers in flight at once. The deny test must also count every path already granted in this run that is not yet widened.
- **Details**: A grant is not reflected in `files` until the drain (no live `tasks.toml` writes), and the Workflow never learns of it. So the second request sees no holder, and neither does the Workflow. The fix belongs in the grant rule and in where grants are stored (request-record lens). The holder set becomes rows plus the run's outstanding grants, keyed by `run_id`.
- **Counter**: The finding fails if grants are written into the holder rows' `files` during the run, or if the request store is consulted by the deny test. Neither is in the brief, and the brief forbids the first.
- **Impact on plan / report**: The grant rule's holder set is "any row of the milestone, plus any path granted earlier in this run". The drain then widens each granted row and orders any two granted rows that share a path (drain.md "A holder").

- **Library/API or file**: tomlctl/src/tasks/schema.rs:714 `normal_path`; tomlctl/src/query.rs:1278 `WhereContains`; tomlctl/src/tasks/graph.rs:422 `first_claim`; versus claude/workflows/implement-v2-milestone.js:157 `fileKey`
- **Source**: code reading at the anchors
- **Evidence-grade**: high
- **Finding**: The path comparison in the deny test must fold case and slashes as `fileKey` does. Every existing tomlctl matcher is case-sensitive, so reusing one lets a case-variant request through while the variant's holder is in flight.
- **Details**: `fileKey` is `replace(/\\/g,'/').toLowerCase()`. `normal_path` folds only backslashes and a leading `./`. `--where-contains` is `s.contains(sub)`, and `first_claim` is a raw `BTreeSet<&str>` intersection. drain.md:204's `tasks list --where-contains files=<path>` has this gap today for the drain's own widening. That widening is sequential, so the gap matters there only for ordering, but a mid-run grant would inherit it as a concurrency hole.
- **Counter**: The finding fails if `files` entries and worker-requested paths are normalised to one canonical case on write, which I found no code doing. It also fails if every target filesystem were case-sensitive, but the user's repos run on Windows.
- **Impact on plan / report**: The grant verb compares with a case-folded, slash-normalised key, using the same rule as the Workflow, and has a test for the `Foo.ts`/`foo.ts` pair. The `case-collision` lens is adjacent but separate: it catches planned collisions, not request-time ones.

Coverage: read in full claude/workflows/implement-v2-milestone.js and docs/flows/mid-run-requests/brief.md. Read in part tomlctl/src/plan/emit.rs (`launch_args`, `Launch::build`), tomlctl/src/plan/records.rs (`run_start`/`finish`/`abandon`), tomlctl/src/plan/lane.rs (`window_open`, `lane_check`, `out_of_lane`), tomlctl/src/tasks/graph.rs (`frontier`, `first_claim`), tomlctl/src/query.rs (`WhereContains`), tomlctl/src/tasks/schema.rs (`normal_path`), plan-worker-lite.md procedure steps 1-7, implement-v2-engine SKILL.md (open-run rule), run-milestone.md (launch and record), drain.md (lane check, widening), and flow-contract-task-store §11. No commands beyond reads and git rev-parse/status. Ruled out: agents.toml as the in-flight source, because hooks write it and the status rule already gives the answer; dead `in-progress` rows as a hazard, because counting them as holders is conservative; a blocked-but-pending row as a hazard, because it only causes a false denial; cross-milestone concurrency, because the open-run rule makes it user-gated.
