Verdict: no restructure. The work extends seams that are already there: a new `plan run` leaf whose grant record lives in plan.toml, the lane check reading the grants, the drain applying them through the audited update it already has, a config module modelled on ui_capture.rs, and two new plan-check classes. One placement is binding: grants must not be written into tasks.toml while the run is live.

```toml sources
read_at = "51fd3b2b88fa092fea7bfc510421e2a6c78f72b7"
dirty = []

[[source]]
path = "claude/workflows/implement-v2-milestone.js"
line = 290
claim = "The pool loop only awaits agent returns. The script has no inbox, so nothing can reach it while a run is live"

[[source]]
path = "claude/workflows/implement-v2-milestone.js"
line = 157
claim = "The Workflow holds files by a slash-normalised, case-folded key"

[[source]]
path = "claude/workflows/implement-v2-milestone.js"
line = 170
claim = "The worker prompt names the return shape and needs_files. It is the only Workflow line the change touches"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 714
claim = "normal_path normalises slashes and a leading ./ but does not fold case"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 462
claim = "The lane check treats every trail row written in the run's window as a write beyond a worker's licence"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 189
claim = "store_digest hashes each section with only the dispatched rows' status and agent blanked"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 423
claim = "out_of_lane builds the lane from row files plus a fixed exempt list"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 357
claim = "The lane check already loads plan.toml and reads its runs"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 246
claim = "While a run is live the orchestrator writes no tasks field on the milestone's rows"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 63
claim = "The lane check runs before any drain store write except the in-progress resets"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 197
claim = "The drain already has a mechanical widening of row files from needs_files, through the audited update"

[[source]]
path = "claude/skills/implement-v2-engine/references/questions.md"
line = 92
claim = "The question loop writes plan.toml and inputs.toml during a live run and never writes tasks.toml"

[[source]]
path = "claude/skills/implement-v2-engine/references/questions.md"
line = 50
claim = "The orchestrator must never answer a worker's question itself"

[[source]]
path = "claude/skills/flow-contract-user-inputs/SKILL.md"
line = 100
claim = "A plan worker may write exactly one question to inputs.toml"

[[source]]
path = "claude/skills/flow-contract-user-inputs/SKILL.md"
line = 109
claim = "Under the trust boundary, an inputs record may never drive a file edit outside its ledger"

[[source]]
path = "tomlctl/src/inputs.rs"
line = 35
claim = "The request kind already exists and means the user's change request against ledger items"

[[source]]
path = "claude/agents/plan-worker-lite.md"
line = 149
claim = "The forbidden-ops shared block forbids plan writes and allows only the one inputs question"

[[source]]
path = "tomlctl/src/plan/dispatch.rs"
line = 31
claim = "The plan run leaves notes, gate and report each live in their own run_*.rs module"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 418
claim = "Attempt is a run-scoped plan.toml record keyed on (task, run_id), the precedent for a grant record"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 191
claim = "The ReportKind vocabulary is where a grant or denial report kind would go"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 185
claim = "gate_list pushes milestone gates, then accept checks, de-duplicating through the listed set"

[[source]]
path = "tomlctl/src/ui_capture.rs"
line = 10
claim = "This is the precedent for a .claude/*.toml project config read by a crate-level module"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 36
claim = "plan check already imports a project config module (ui_capture) for its classes"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 211
claim = "FileKind::New marks a row's file as one it creates, read from its Files note"
```

## Why the present structure holds

Every point where the future state touches the code is a seam that already exists. Followed through, none of the forced edits reaches across a module boundary. The work is placement, not reshaping. The one placement that would force a reshape if done the other way is claim 2.

### 1. The Workflow script stays as it is

`implement-v2-milestone.js` has no way to hear a worker mid-run. Its loop only awaits `agent()` promises (`implement-v2-milestone.js:290-305`), and its hold set is fixed from args at `start` (`:273-274`). The grant rule says "no other row of its running milestone holds it". That means no args task holds the granted path, so the hold set cannot conflict with a grant, and no dispatch logic moves. The edit this change forces here is the prompt text at `:170` (and possibly the comment at `:34-36`). The request loop runs between the worker and the main conversation, as the question loop already does (`run-milestone.md:252`).

One thing this does force: the grant check must compare paths the way the Workflow does, slash-normalised and case-folded (`implement-v2-milestone.js:157`). Rust's `normal_path` does not fold case (`tomlctl/src/tasks/schema.rs:714-716`). The case-collision warning needs the same folded key, so add one folded-key helper beside `normal_path`. Do not change `normal_path` itself, because lane, binds and file holders all compare through it.

Refuted if: the grant rule were "no running task holds it" rather than "no row of the milestone". A pending row of the same launch could then hold the granted path, and the Workflow would have to learn about grants. That would need an inbox it lacks.

### 2. Grants live in plan.toml and land on tasks.toml only at the drain

A grant written to tasks.toml while the run is live would be reported as a breach on two paths:

- The trail check counts every `[[changes]]` row inside the run's window as a write beyond a worker's licence (`lane.rs:462-471`).
- `store_digest` blanks only the dispatched rows' `status` and `agent` (`lane.rs:189-202`). A widened `files` therefore moves the `rows` section and a corrected accept clause moves `specs`.

The engine also forbids the write outright (`run-milestone.md:246`, `drain.md:7`). Licensing it would mean projecting the granted fields out of the digest by a grant list, which you need anyway, or exempting trail rows by `actor`, which is self-declared.

plan.toml is already the store that may be written during a live run (`questions.md:92`), and the lane check already loads it (`lane.rs:357-375`). So the request and its verdict belong there:

- **Record:** a run-scoped plan.toml record keyed on `(run_id, task)`, shaped like `[[attempt]]` (`schema.rs:418-437`).
- **Leaf:** written by a new `plan run` leaf in its own module (for example `plan/run_request.rs`), wired in `dispatch.rs:31-34` as `run_notes`, `run_gate` and `run_report` are.
- **Leaf checks:** the leaf runs the mechanical half of the rule:
  - holders among the milestone's rows, case-folded;
  - paths already granted to another task in this run;
  - forbidden paths (`.claude/`, `docs/flows/<slug>/`, by `drain.md:199`);
  - declared outputs of a held input.
- **Run record:** this record is the run record the future state names. The milestone report takes a report kind added to `ReportKind` (`schema.rs:191-202`).

Refuted if: a tasks.toml write made between `plan run start` and `plan run finish` left both `trail_writes` and the digest unchanged. Read with the code above, it cannot.

### 3. The lane check reads the run's grants

`out_of_lane` builds the lane from the rows' `files` plus a fixed exempt list (`lane.rs:423-442`). The drain runs the lane check before any store write of its own (`drain.md:63`). A granted file that a worker edited would therefore be reported `out-of-lane`, because the row is not widened until later. The forced edit: insert the run's granted paths into `lane`. This also covers an abandoned run settled at re-entry, since the grants are on disk.

Refuted if: the drain could widen rows before the lane check without moving the digest. Claim 2 shows it cannot.

### 4. The drain applies grants through the update it already has

The gate list reads accept checks from the stored spec (`emit.rs:201-230`), and the milestone commit stages files by the rows' `files`. Both force the grants onto tasks.toml before step 7, including grants on `done` rows. The mechanism already exists:

- the audited `tasks update --ndjson` (`drain.md:170-195`);
- the "Widening a row from `needs_files`" rules (`drain.md:197-222`), applied here without the relaunch.

The edit is a new input row in the step-6 table (`drain.md:146-158`) with the grant id in `reason`. It is not a new write path.

### 5. The request loop sits beside the question loop, and inputs.toml is not its home

`questions.md` stays as it is. A new sibling reference, `requests.md`, is linked from `run-milestone.md:252` and `SKILL.md:73`.

inputs.toml is ruled out as the request's home:

- The trust boundary forbids any inputs record from driving a file edit (`flow-contract-user-inputs/SKILL.md:109-111`).
- A worker's inputs licence is one question (`:100`).
- The `request` kind already means the user's ledger change request (`inputs.rs:35`, `SKILL.md:73` of that contract).
- A worker-posted `question` that the orchestrator answers by rule breaks `questions.md:50` and contract `:103`.

An escalation ("changes what the work is for") goes back to the worker as a denial. The worker then uses its existing one question under **Asking the user**. The question loop does not change.

Whichever side writes the request record, the forbidden-ops shared block (`plan-worker-lite.md:149`, mirrored in deep) changes in one line: it gains either that one verb or an exception for a granted file. The lite worker's own `failed` list (`plan-worker-lite.md:136-138`) and the deep worker's Judgement bullets on stale detail and file boundary each change one bullet. These are edits inside existing sections.

### 6. generated.toml follows the ui-capture precedent

- A crate-level `tomlctl/src/generated.rs` holds a `CONFIG` constant and a loader returning `Result<_, String>`, as `ui_capture.rs:10-40` does.
- It is consumed by `plan check`, which already imports a config module that way (`check.rs:36`), by the grant leaf (claim 2), and by `gate_list`.
- In `gate_list`, the generator check commands push into `commands` through the same `listed` de-duplication (`emit.rs:185-194`), with their own `source`.

"Touched" can be read statically, as the milestone rows' `files` intersecting a declared input, so the leaf needs no git call.

### 7. The case-collision class reads what the store already says

"Creates a file" is already in the store as `FileNote` plus `FileKind::New` (`tasks/schema.rs:193-235`, used at `tasks/show.rs:221`). Existing files come from `repo_files::tracked_files`. The class is one more function in `check.rs` with the folded key from claim 1.

## Alternatives ruled out

- **Grants written straight to tasks.toml, licensed by trail `actor`:** `actor` is self-declared, and the digest still moves (claim 2).
- **The Workflow tracking grants:** it has no inbox (claim 1).
- **Requests riding inputs.toml, or the question loop answered by rule:** the trust boundary, the worker licence and `questions.md:50` all refuse it (claim 5).
- **Merging `questions.md` and the new loop into one "worker messages" reference:** no edit forces it. The question loop's steps are specific to the user picker.
- **Changing `normal_path` to fold case:** it would change every comparison in lane, binds and holders, a wider edit than this work needs.

## Coverage

- **Read in full:** `claude/workflows/implement-v2-milestone.js`, `claude/agents/plan-worker-lite.md`, `claude/skills/implement-v2-engine/references/run-milestone.md`, `references/questions.md`, `tomlctl/src/plan/run_report.rs`.
- **Read in part:**
  - `tomlctl/src/plan/lane.rs` (1-579): digest, window, `out_of_lane`, `trail_writes`.
  - `references/drain.md` (1-370).
  - `tomlctl/src/inputs.rs` (1-110 and 560-720): kinds and validation.
  - `tomlctl/src/plan/schema.rs` (185-545): `PlanStore`, `Run`, `Attempt`, `Report`, `ReportKind`.
  - `tomlctl/src/plan/emit.rs` (`gate_list`).
  - `tomlctl/src/plan/check.rs`: header and function index.
  - `tomlctl/src/plan/dispatch.rs`, `tomlctl/src/ui_capture.rs` (head), `tomlctl/src/repo_files.rs` (head).
  - `tomlctl/src/tasks/schema.rs`: `FileNote`, `FileKind`, `normal_path`.
  - `claude/skills/flow-contract-user-inputs/SKILL.md` (50-176).
  - `claude/agents/plan-worker-deep.md`: head and Judgement.
  - `claude/skills/implement-v2-engine/SKILL.md`, by grep.
- **Not read:** `tomlctl/src/plan/view.rs`, `tomlctl/src/plan/records.rs` beyond its header, `tomlctl/src/tasks/check.rs`, `claude/skills/plan-v2-engine/`, and `references/apply-rows.md` past its opening.
