Verdict: Grants must be written to `tasks.toml` at one new drain point after step 5 and before step 6's other replan rows (the lane check at drain.md:63 forbids any earlier write, and `plan emit gate-list` reads only the store, emit.rs:172), with `plan emit lane-check` (lane.rs:423) and the pre-flight clean-tree check reading the granted paths themselves, since nothing is in `files` until that write; apply-rows.md:225 has to stop treating a denied scope request on a fix row as a tier failure.

```toml sources
read_at = "51fd3b2b88fa092fea7bfc510421e2a6c78f72b7"
dirty = ["docs/flows/mid-run-requests/brief.md"]

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 63
claim = "The lane check runs before any drain store write except step 4's resets, because every other write moves the digest it compares"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 31
claim = "A done row gets no drain action unless it has deviations or a handover"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 108
claim = "The step-3 audit calls gate-list and allows only checks that match a row's accept or detail"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 136
claim = "Step 5 explains digest changes by diffing tasks.toml against its last commit, so a write before step 5 shows up in that diff"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 195
claim = "Each spec field an update supplies replaces the stored one whole; the batch is all-or-nothing"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 210
claim = "A needs_files widening adds holders to the widened row's needs"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 298
claim = "The gate list is emitted at step 7 from the store"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 546
claim = "The report's section-to-kind table"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 423
claim = "The lane is the files of dispatched or started-uncommitted rows plus fixed exempt stores, and nothing else"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 189
claim = "The digest covers rows and specs with only the dispatched rows' status and agent blanked, so a files or spec write moves it"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 172
claim = "gate-list builds commands only from the stored milestone gates and spec.accept owners"

[[source]]
path = "tomlctl/src/tasks/update.rs"
line = 232
claim = "An update batch refuses a second row for the same task"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 191
claim = "ReportKind is a closed vocabulary"

[[source]]
path = "tomlctl/src/plan/run_report.rs"
line = 79
claim = "plan run report refuses a kind outside ReportKind"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 32
claim = "The pre-flight clean-tree exemptions are a fixed list, and started rows' files is one of them"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 112
claim = "An abandoned implement run gets only the lane check (steps 3 and 5) at re-entry, not the drain's replan"

[[source]]
path = "claude/skills/implement-v2-engine/references/apply-rows.md"
line = 225
claim = "Failure on an apply row replaces step 6 for failed or parked apply rows, needs_files widening included"

[[source]]
path = "claude/skills/implement-v2-engine/references/apply-rows.md"
line = 157
claim = "Every fix row carries the same milestone-wide reproduce-status accept, run by the gate"

[[source]]
path = "docs/flows/mid-run-requests/brief.md"
line = 11
claim = "Non-goal: no tasks.toml write while a milestone run is live"
```

## Answer

Nothing may touch `tasks.toml` while a run is live (brief.md:11). After the run, the drain's lane check must come before any write except the step 4 resets (drain.md:63). So a granted path or spec change cannot be in the store when the lane check reads it. That leaves four places to change:

1. **The lane check has to read granted paths itself.** `out_of_lane` (lane.rs:423-442) builds the lane from the rows' `files` and a fixed list of exempt stores. A path granted to a dispatched or started row has to join that lane, read from wherever the request records live (that is the request-record lens). The pre-flight clean-tree exemptions (run-milestone.md:32-38) need the same entry. Without it, a session that ends after `plan run finish` but before step 6 leaves a granted edit that the next invocation stops on as "someone else's edit". An abandoned run is in the same position, because re-entry runs only the lane check for it (SKILL.md:112). The digest needs no change, since grants write nothing to it during the run.
2. **Apply grants in one new step, "Granted requests", between step 5 and the other step-6 rows.** It cannot come earlier. Step 5 explains digest changes by diffing `tasks.toml` against HEAD and telling apart the rows the run dispatched (drain.md:136), and grant writes land on exactly those rows. It cannot come later either. Step 6's re-spec rule reads the current spec and re-issues each list whole (drain.md:195), so a re-spec written over a stale spec would silently drop an amend. The step covers every task that holds a grant, whatever its status. Today a `done` row has no drain action (drain.md:31), so step 2's table needs a "holds a granted request" clause.
3. **Each grant is an audited `tasks update --ndjson` row.**
   - The `reason` opens with a prefix of its own, such as `granted scope <input>:` or `granted amend <input>:`. The prefix must not start with any of the counted ones: `gate <id> round `, `reproduce <id> round `, `apply failure `, `carried from gate `.
   - The batch refuses two rows for one task (update.rs:232). A task with both a scope and an amend grant, or a grant plus a step-6 re-spec, therefore needs one row whose reason names each request. The other way is to apply the grants as their own batch before the replan batch.
   - A scope grant re-issues `files` whole, as needs_files widening does (drain.md:209). The ordering is the reverse of that widening, though. The widened row was dispatched, so every row it `needs` is done and it has already edited the path. A pending holder therefore gains the granted row in its own `needs`. This cannot form a cycle, because the holder cannot be an ancestor of a dispatched row.
4. **Amends have to reach the store before the gate list is printed.** `gate-list` reads only stored `milestones.gates` and `spec.accept` owners (emit.rs:172-231), and it is called twice:
   - at step 7, which the new step precedes;
   - in the step-3 audit (drain.md:108-111), which also allows only checks matching a stored `accept`.

   The audit writes nothing, so the simplest fix is to move it after "Granted requests". Otherwise a worker that ran its granted accept check is reported as a breach. The reproduce check and settle drain run at the end of step 6, after the new step, so they also see the amended spec.
5. **The report and the run record need a new report kind.** `ReportKind` is a closed vocabulary (schema.rs:191) and `plan run report` refuses any other kind (run_report.rs:79). Adding it means:
   - a new kind, for example `request`, with a `requests:` section in the report shape and the kind table (drain.md:494-557);
   - `needs_person = false` on grant lines, since they only inform;
   - a denial line that also informs, because the row's `failed` line already asks for the person;
   - matching edits to `plan-run.md` and to `tomlctl/tests/plan_run_records.rs`.
6. **apply-rows.md, "Failure on an apply row" (apply-rows.md:225-254).**
   - That section replaces step 6 wholesale for failed apply rows, so a fix row that failed with `needs_files` today moves `lite` to `deep` with its `files` unchanged. It then fails again for the same reason.
   - Under requests, a granted scope or amend is not a failure. It counts toward no `apply failure <n>`; the prefix in point 3 keeps it out of the count at apply-rows.md:234.
   - A denied scope request that comes back `failed` with `needs_files` should take drain.md's needs_files widening, including its park rules, instead of the tier table.
   - The section should say what a fix row's amend may not do. It may not touch the milestone-wide `review reproduce-status ... --expect gone` accept (apply-rows.md:157-165). It may not change the outcome away from "finding gone", which is reclassifying (apply-rows.md:229). Either request is denied, and the row follows the tier table.

## Findings

- **Library/API or file**: tomlctl/src/plan/lane.rs:423 `out_of_lane`; claude/skills/implement-v2-engine/references/drain.md:63
- **Source**: lane.rs:426-442 (lane = run.tasks rows' files ∪ started-uncommitted rows' files ∪ backlog/inputs/finding-link stores); drain.md:63
- **Evidence-grade**: high
- **Finding**: `plan emit lane-check` must read granted scope paths from the request records. The drain cannot widen `files` before the check without itself showing up as a digest `rows` change.
- **Details**: `store_digest` (lane.rs:189-202) blanks only the dispatched rows' `status`/`agent`, so a `files` write before the check reads as `{source: "digest", section: "rows"}`. The fix adds each path granted to a row in `run.tasks`, or to a started row with no commit, into `lane`, next to the existing exempt stores. The lane-check docs in drain.md:71 and plan.md change with it. A unit test goes in lane.rs tests.
- **Counter**: Wrong if the request-record lens chooses to write granted paths into some field that `out_of_lane` already reads. It cannot be `tasks.toml`, because of the brief's non-goal.
- **Impact on plan / report**: A tomlctl task on lane.rs, together with the request-record reader.

- **Library/API or file**: claude/skills/implement-v2-engine/references/run-milestone.md:32 (clean-tree exemptions); SKILL.md:112
- **Source**: run-milestone.md:30-40; SKILL.md:112 (an abandoned run gets only the lane check)
- **Evidence-grade**: high
- **Finding**: The pre-flight clean-tree check needs an exemption for paths granted to a started row and not yet applied. Otherwise a session that ends between the notification and step 6 stops the next invocation on its own granted edit.
- **Details**: A granted path is in no row's `files` until the drain applies it, so line 40's "any other path is someone else's edit" stops the engine. Two fixes work: add a bullet to the exemption list, or have re-entry apply outstanding grants before the pick and say so at SKILL.md:112.
- **Counter**: Wrong if outstanding grants are always applied before the pre-flight's tree check runs. Re-entry's order (SKILL.md:90, "after the pre-flight's stage check") does not establish that.
- **Impact on plan / report**: A prose change in run-milestone.md and SKILL.md Re-entry.

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:136, drain.md:195, drain.md:31
- **Source**: drain.md:136 (step 5 diffs tasks.toml against its last commit); drain.md:195 (spec lists re-issued whole); drain.md:31 (`done` → nothing)
- **Evidence-grade**: high
- **Finding**: Add a "Granted requests" step after step 5 and before the other step-6 rows. It applies every granted scope and amend to its row through audited `tasks update --ndjson`, including rows that are `done`.
- **Details**: Writing before step 5 adds noise to its diff, on the very rows it uses to tell writes apart. Writing after a step-6 re-spec would overwrite the amend, because `constraints`, `accept` and `means` are replaced whole. Step 2's table gains "or holds a granted request" for `done`. The relaunch-or-stop rule (drain.md:289) counts a grant write as "a field moved", so it needs a carve-out: a grant must not keep relaunching a row that made no progress.
- **Counter**: Wrong if step 5 is changed to read the trail alone, so it no longer needs a clean diff. Grant writes before it would then be harmless.
- **Impact on plan / report**: One drain.md task; it orders against the worker-protocol lens only through the request record's fields.

- **Library/API or file**: tomlctl/src/tasks/update.rs:232
- **Source**: update.rs:232 `"task {id} is already patched by line {first} — give each task one row"`
- **Evidence-grade**: high
- **Finding**: A task carrying more than one grant, or a grant plus a re-spec, must be one update row with a combined `reason`, or go in separate batches.
- **Details**: Each `[[changes]]` row stores `field/old/new/actor/reason` (tasks/schema.rs:430-437), so one reason per task row is what the trail keeps. The reason prefix must avoid the counted prefixes `gate <id> round ` (drain.md:385), `reproduce <id> round ` (apply-rows.md:174) and `apply failure ` (apply-rows.md:234).
- **Counter**: Wrong if the batch accepted repeated ids; update.rs:232 shows it does not.
- **Impact on plan / report**: Fixes the shape of the drain's grant record file.

- **Library/API or file**: tomlctl/src/plan/emit.rs:172 `gate_list`; drain.md:108
- **Source**: emit.rs:185-231 (commands only from `spec.accept` owners and milestone gates); drain.md:108-111 (the audit's `--gate` list and allowed rule read stored accepts)
- **Evidence-grade**: high
- **Finding**: An amend's accept, means and constraints are in the store before both gate-list calls only if the step-3 agents audit moves after "Granted requests", or its allowed rule also reads granted amends.
- **Details**: The audit only reads, so moving it costs nothing. Step 7's gate list then sees the amended `run_by`/`writes_build` owners, and the reproduce check and settle drain at the end of step 6 run on the amended spec. `gate-list` itself needs no code change for amends. The generated-map lens's generator check commands are a separate gate-list input.
- **Counter**: Wrong if an amend may never change `accept`. The brief's success criterion 2 lets an accept clause be dropped or loosened, so it may.
- **Impact on plan / report**: A drain.md reorder; no tomlctl change for this part.

- **Library/API or file**: tomlctl/src/plan/schema.rs:191 `ReportKind`; tomlctl/src/plan/run_report.rs:79
- **Source**: run_report.rs:79-87 refuses an unknown kind with the vocabulary list
- **Evidence-grade**: high
- **Finding**: Grants and denials in the milestone report and run record need a new `ReportKind` value, for example `request`, plus a `requests:` section in drain.md:494-557.
- **Details**: Lines carry `task` and `needs_person = false`, since they only inform. The vocabulary, `plan-run.md`, `plan-store.md` and the `plan_run_records` test change together. `glimpse/src` has no match for the report kinds, so glimpse probably needs no change; I did not confirm that.
- **Counter**: Wrong if grants were reported under an existing kind such as `reconcile`. That would mislabel them, and `reconcile` lines need a person by default.
- **Impact on plan / report**: A tomlctl vocabulary task plus drain.md prose.

- **Library/API or file**: claude/skills/implement-v2-engine/references/apply-rows.md:225 "Failure on an apply row"
- **Source**: apply-rows.md:227 ("These rules replace drain.md step 6's for a failed or parked apply row"); apply-rows.md:239-242 tier table; drain.md:151
- **Evidence-grade**: medium (reading of how "replace" covers needs_files widening; no run evidence)
- **Finding**: A denied scope request on a fix row should go through drain.md's needs_files widening and park rules, not the lite→deep tier move. Granted requests should never count as `apply failure <n>`.
- **Details**:
  - Today a `lite` fix row that fails with `needs_files` moves to `deep` with its files unchanged, and fails again for the same reason.
  - A fix row's amend may not remove or loosen the milestone-wide `reproduce-status --expect gone` accept (apply-rows.md:157-165). It may not change the outcome away from "finding gone", which is reclassifying (apply-rows.md:229). Either request is denied, and the row then follows the tier table.
  - `review schedule`'s two-file `lite` rule (schedule.rs:762) applies only when rows are minted, so a scope grant that widens a lite row needs no executor change.
- **Counter**: Wrong if the needs_files widening in drain.md:151 was meant to keep applying to apply rows despite "replace". The apply-rows text says otherwise.
- **Impact on plan / report**: An apply-rows.md task, ordered after the drain.md grant step it cites.

Out of lens, one line: whether a granted path can also be held by a concurrent in-flight row is the live-run-holders lens. This note assumes the grant rule excludes that case.

Coverage: read in full drain.md, apply-rows.md, questions.md, brief.md and lane.rs:1-490; read in part run-milestone.md:1-45, SKILL.md:86-121, emit.rs:165-241, run_report.rs:60-110, schema.rs:170-203 and schedule.rs:750-775; grepped update.rs for the duplicate-id refusal and reason handling, and glimpse/src for report-kind rendering (no match). Ruled out:
- a store-digest change, since grants write nothing during the run;
- `plan drift --uncommitted`, which needs no change because it lists paths and affected rows only;
- step 10's commit staging, which is already correct once grants are in `files`.
