Verdict: Milestone A cannot be carried out as written. Two points stop it: task 7 cannot reach `plan::authored` from lib.rs because task 1 declares it private, and task 12 names a `review show` "human" role that does not exist. The other twelve tasks are actionable.

## Unactionable

### U1. Task 7: lib.rs cannot call into `plan::authored` as task 1 declares it (blocking)

- Task 1, Action: "declare each in tomlctl/src/plan/mod.rs with a `mod <name>;` line beside the existing private modules". This makes `authored` a private child of `plan`. Task 1 changes only `probes` to `pub(crate)`.
- Task 7, Action: "Write `authored_list(root, slug)` and `authored_read(root, slug, path)` in authored.rs and expose both as `pub fn` in lib.rs". Task 7's files are `plan/authored.rs`, `lib.rs` and `tests/library_facade.rs`. Its constraints are "Touch no file outside this task's files", and task 1 adds that "Later tasks fill each file and add no `mod` line".
- The code: lib.rs declares `mod plan;` (tomlctl/src/lib.rs:30), so lib.rs is the crate root and the parent of `plan`. A private `mod authored;` inside `plan` is visible only within `plan` and its descendants. A `crate::plan::authored::authored_list` call from lib.rs fails with E0603 (module `authored` is private). A `pub(crate) use` re-export would also need a line in plan/mod.rs.
- What is missing: either task 1 declares `pub(crate) mod authored;`, as it already does for `probes`, or task 7 owns plan/mod.rs for one visibility change or re-export. With the plan as written, the worker has to break a constraint or guess.
- The other new tomlctl modules are fine. `run_note`, `note_sources`, `run_notes`, `run_gate` and `run_report` are reached only from inside `plan`: by `dispatch.rs`, `authored.rs` and `run_notes.rs`.

### Minor ambiguities (not counted; a worker can pick something reasonable)

- Task 6, Detail: it returns sources as `{path, line, claim, url, read_at, dirty, origin}`. Task 6's view never says what `origin` holds or what an inline source's `claim`, `url` and `read_at` are. Only task 7's design (Authored files) says `origin` is `block` or `inline` and shows `""` for the empty strings. Task 7 adapts whatever task 6 returns, so the risk is low, but the task 6 worker is guessing.
- Task 9, Detail: the log file name `<milestone>-r<round>-c<n>.log`, with "`n` being the command's position in the round". The view does not say whether `n` is 0-based or 1-based. The sample `B-r0-c2` does not settle it. Nor does it say whether a re-record that replaces one command renumbers by its position in the incoming batch or in the merged round. It also does not say what a relative `log` path in a row resolves against: the cwd or the repo root.
- Task 12, Detail: "declare the module with one `mod facade;` line in review/mod.rs". Taken literally, `mod facade;` is private to `review`, and lib.rs could not call it, for the same reason as U1 (lib.rs:34 `mod review;`). Task 12 owns review/mod.rs, so `pub(crate) mod facade;` still satisfies "one mod line" and the accept grep. A worker who copies the backtick text will hit E0603 first.
- Task 2: the design's snapshot accept entry holds `owner` and `probeable`, and always `run_by` and `covered_by`. `show::accept_json` (tomlctl/src/tasks/show.rs:143-174) emits no `owner` and leaves out `run_by` and `covered_by` when they are empty. The constraint says "`tasks show` output changes only by a premise gaining `probe`". So the worker has to add `owner` and `probeable` in a snapshot-side wrapper rather than in `accept_json`. The task does not say so, but a careful reader can work it out.
- Task 7, Detail: "Canonicalise each file ... as `store::locate_brief` does". `locate_brief` anchors through `authored_anchor`, which calls `repo_or_cwd_root()`, the process root (tomlctl/src/plan/store.rs:62-75). Copying it as it stands would break task 7's "read no process-global root". The worker has to canonicalise against `root` instead. The task warns about `authored_dir` but not about this.

## False premises

### F1. Task 12: `review show` has no "human" role

- The claim (task 12, Action): it "returns one finding of a v2 review store as the object `review show` prints for the human role". The design: "as the object `review show --for human` prints for it". The Detail: "Reuse the projection `review show` already builds for the human role".
- What I read: the projection roles are `finder`, `verifier`, `checker`, `implementer`, `orchestrator`, `audit`, `backlog` and `report` (tomlctl/src/review/fields.rs:93-105, `str_enum!(Role { ... })`). `review show` parses `--for` through `Role::parse` and refuses anything else (tomlctl/src/review/query.rs:275-282). claude/skills/tomlctl/references/review.md:136 says "`list` and `show` project every row through one of eight roles". `rg -i human tomlctl/src/review/` finds nothing.
- What a worker would need: which existing role the facade projects for. `report` or `orchestrator` are the likely intents. The reuse itself is feasible within task 12's files: for a role other than `verifier`, `audit` and `backlog`, `query::finding_row` reduces to `View::new(&store).project(role, index, true)`. That is `pub(super)` in review/store.rs:551-601 and so reachable from `review::facade`.

All other premises hold; see the checks below.

## Check commands

Premises at base, run from the repo root. Each printed nothing and exited 0 (HOLD):

- T1 `grep -q "^mod probes;" tomlctl/src/plan/mod.rs`: HOLD
- T2 `grep -q "fn task_json(row: &TaskRow)" tomlctl/src/tasks/snapshot.rs`: HOLD. `grep -q "fn skip_reason(accept: &Accept)" tomlctl/src/plan/probes.rs`: HOLD. `premise_skip_reason` exists as a private fn taking `(&Store, &Premise)` (probes.rs:193); task 2 owns probes.rs and can raise its visibility.
- T3 `grep -q "running_milestone" tomlctl/src/tasks/snapshot.rs`: HOLD. The plan parse is at snapshot.rs:105-108.
- T4 `grep -q "schema_version > SCHEMA_VERSION" ...`: HOLD. `grep -q "pub(crate) runs: Vec<Run>" ...`: HOLD. `SCHEMA_VERSION = 2` (plan/schema.rs:27).
- T7 `grep -q "pub fn snapshot_if_changed" tomlctl/src/lib.rs`: HOLD. `grep -q "fn locate_brief" tomlctl/src/plan/store.rs`: HOLD.
- T8 `PlanRunOp` enum and the `records::run_start` arm: HOLD. `pub(crate) tasks: Vec<u32>`: HOLD. `Run` also has `run_id` (plan/schema.rs:254). plan.md has `## \`plan run skip\`` at line 258.
- T11 `tokens_in` and `context_tokens`: HOLD. `Event::Stop` is built in record.rs:517-600, inside the task's files.
- T12 `grep -q "^mod query;" tomlctl/src/review/mod.rs`: HOLD.
- T13 `FEATURES` const: HOLD.
- T14 `^pub(crate) mod details;` in view/mod.rs: HOLD. `^mod mode;` and `^mod scan;` in source.rs: HOLD. main.rs has the alphabetical `mod` list (lines 3-31). glimpse/src/model/ does not exist yet; model.rs declares no modules besides its tests.

Forward accept greps on the present tree. Each FAILS, as it should before the change:

- T1 `mod run_note;` and `pub(crate) mod probes;`: FAIL
- T2 `"milestones"` with `"changes"`, and `probeable` in snapshot.rs: FAIL
- T3 `"plan"` in snapshot.rs: FAIL (`"plan_path"` does not match). `file_notes` with `probeable` in tasks-store.md: FAIL
- T4 the full conjunction: FAIL. Non-discriminating sub-clause: `grep -q "report" tomlctl/src/plan/schema.rs` already HOLDS at base, so that part proves nothing. `attempt` (FAIL now) and `gate_round` (FAIL now) carry the check. The plan-store.md check: FAIL.
- T8 `Notes` in cli/types/plan.rs: FAIL. `plan run notes` in plan.md: FAIL
- T9 `PlanRunOp::Gate` and `plan run gate`: FAIL
- T10 `PlanRunOp::Report` and `plan run report`: FAIL
- T11 `cache_write` in agents/schema.rs and in agents.md: FAIL
- T7 and T12 `pub fn authored_list` and `pub fn review_finding` in lib.rs, and `mod facade;` in review/mod.rs: FAIL
- T13 `snapshot_plan_records` in cli/types/mod.rs and `plan_run_records` in README.md: FAIL
- T14 `^mod nav;` in main.rs, `mod plan;` in model.rs, `^mod doc;` in source.rs, `mod links;` in view/mod.rs: FAIL

Not run: every `cargo nextest` accept (tasks 2-13). Each writes the build, is owned by the gate and is covered by the full suite. Running them here would take the shared build lock.

Anchors checked: tasks-store.md has "The envelope's keys, in emission order" (line 234). agents.md documents `context_tokens` (line 178). plan-store.md has the store sample and key tables. tomlctl/README.md has the capabilities sample block (from line 331) and the feature table (from line 460). The `.toml` record-file form of `--ndjson` exists (tomlctl/src/io.rs:311, src/payload.rs). The `every_leaf_command_has_a_case` test that tasks 8-10 must satisfy exists in tests/output_options.rs:2334. One observation: tests/capabilities.rs:132-190 keeps a hand list of write-verb `--help` paths, including `plan run skip`, that no task updates. As far as I can see it is not exhaustive, so leaving the new leaves off it should not fail anything.

## Coverage

Tasks read: 1-14, each through `tomlctl plan view --for worker` and `tomlctl tasks show --select id,files`. Code read: tomlctl/src/plan/{mod.rs,probes.rs,schema.rs,store.rs,dispatch.rs}, tomlctl/src/tasks/{snapshot.rs,show.rs,schema.rs}, tomlctl/src/cli/types/plan.rs, tomlctl/src/lib.rs, tomlctl/src/review/{mod.rs,query.rs,fields.rs,store.rs}, tomlctl/src/agents/{correlate.rs,record.rs}, tomlctl/tests/{capabilities.rs,output_options.rs,tasks_snapshot.rs}, and the glimpse module declarations (main.rs, view/mod.rs, source.rs, model.rs). Not checked: the shape of a real Claude transcript's `message.id` and `usage` lines, which task 11 tells its own worker to confirm. Any cargo build or test.
