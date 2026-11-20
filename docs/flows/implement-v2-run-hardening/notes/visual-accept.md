Verdict: Nothing in tomlctl records a visual round or verdict or gates the commit. The round is counted from `visual <id> round ` reasons in tasks.toml (claude/skills/implement-v2-engine/references/visual.md:28), and the stop before commit is prose only (drain.md:438, visual.md:182). So `plan run visual --accept` needs a new `[[visual_accept]]` plan-store array, keyed by milestone and round and holding the waived states, the look entries and a backlog id, with a schema-version tier. The readers that change are prose: visual.md, drain.md step 10 and its re-entry rule, SKILL.md and report.md. The report line reuses kind `gate`.

## Answer

**How rounds are recorded and counted today.**
- **Counting.** Visual rounds are counted only from the `reason` prefix `visual <id> round <r>: ` on the fix tasks the pass mints, through `tomlctl items list ... --array changes --where-prefix "reason=visual <id> round "` (visual.md:28-31). The round `r` is the highest number printed plus one.
- **Run row.** The run row is `[[run]]` with `step = "visual:<id>"`, written by `plan run start/finish/abandon` (visual.md:89, 108-109). `Run` has no round field and no verdict field (tomlctl/src/plan/schema.rs:344-372).
- **Where the round lives.** It survives only in the args file, which stays in scratch and is not kept, in the note paths `visual-<id>-<r>-<n>.md`, in the evidence directory `notes/run/visual-<id>-<r>/` and in the minted reasons.
- **Verdicts.** They live only in the inspector notes. The pass is kept out of `plan run gate` on purpose (visual.md:47).
- **Code.** No tomlctl code parses round reasons; a search for round-reason parsing found none.

**How the commit is gated.** The gate is prose only:
- drain.md:438 runs the commit "once its visual pass has passed".
- SKILL.md:61 lists that as the commit's skip condition.
- visual.md:182-188 says that after round 2 the pass "mints nothing: leave the milestone uncommitted".
- `tasks update --commit` and every other verb accept a commit regardless.

So nothing in the binary has to change for the commit step to proceed. The change is in the prose, plus a record the prose reads.

**What happens on the next invocation.** A milestone left uncommitted reads as `unstamped` in the schedule (tomlctl/src/tasks/milestones.rs:188-191). The re-entry rule then re-runs the gate and then the visual pass, its round counted from the reasons (drain.md:483, SKILL.md:118). With no acceptance it captures and fails again, and stops at the same place every time. An acceptance given in a later session is therefore read on this re-entry path, not only in the session that hit the failure.

**`plan run report`.** It appends free-text `[[report]]` entries whose `kind` must come from `ReportKind` (schema.rs:197-212, run_report.rs:79). tomlctl reads them nowhere else. glimpse renders the latest one (glimpse/src/model/plan.rs:160-162; surface.rs:862 filters on `needs_person`). The printed report is the orchestrator's template (report.md:28-33).

**What the record must hold.** A new plan-store array, for example `[[visual_accept]]`:
- `milestone`
- `round`: the pass whose verdicts are waived, so `<out>/<id>-<r>/` and `notes/run/visual-<id>-<r>/` resolve.
- `at`
- `why`: the user's words.
- `states`: non-empty, each in the milestone's `ui_states`, never a control.
- `looks`: a list of `{task, accept}`, each a `run_by = "look"` entry on a row of the milestone, the same keys `gate-list`'s `visual.looks` prints (emit.rs:219-226).
- `defects`: a list of `{state, class, location, note}`, copied from the index so that the backlog item and the report need no notes re-read.
- `backlog`: the id of the follow-up item. Mint the item first, so the record is a single write.

Where the verb can check things:
- It can check `states` and `looks` against the tasks store.
- It cannot check that those states actually failed, since no verdict is stored. That stays the engine's to assert, and so does refusing an unsound run or a capture failure, neither of which has verdicts to waive.

**Readers that must change.**
- **visual.md.**
  - "Two rounds at most" (182-188): ask the user, run backlog capture, then `plan run visual --accept`, then go to the commit.
  - Step 2 / Re-entry (25-31, 196): on re-entry, an acceptance for `<id>` at the current `r` means either skip the pass, or re-run it and treat a `fail` limited to waived states and looks as a pass. The planner has to pick one.
- **drain.md.**
  - Step 10's entry condition (438).
  - "Stamping on re-entry" (483).
  - "Next" (487), which lists "visual failed after round 2" as reaching the report without a commit.
- **SKILL.md.** The step table rows 58 and 61, and Unstamped milestones (118).
- **report.md.** A `gate:` line such as `- milestone <id> visual round <r> accepted: <states>; <looks> — backlog <B-id>`, plus the note on `needs_person` (85). An accepted waiver only informs, so `needs_person = false`. Reusing kind `gate`, as the other visual lines do (report.md:76), avoids a `ReportKind` change. A new kind would make an older binary fail with a vocabulary error (schema.rs:1367) rather than the version refusal.
- **tomlctl.** Schema plumbing for the new array:
  - Add `PlanStore`, `from_toml` and `to_toml`, and a `write_version` tier above 5 (schema.rs:35-49, 233-251, 546-559, 600-662).
  - Update every `PlanStore` literal.
  - Add a `PlanRunOp` leaf (cli/types/plan.rs:133-151) and its dispatch arm (plan/dispatch.rs:31-42).
  - Add a feature string in cli/types/mod.rs:210-230 and the carrier's pre-flight list (claude/commands/implement-v2.md:29).
  - Add an `output_options.rs` case and the flag table in the `tomlctl` skill's plan reference.
  - glimpse's serde model ignores the new key, so it needs no change unless the waiver is to be shown.

## Findings

- **Library/API or file**: claude/skills/implement-v2-engine/references/visual.md:28 (round count) and tomlctl/src/plan/schema.rs:344 `Run`
- **Source**: visual.md:25-31, 47, 89; schema.rs:344-372; searches for round-reason parsing in tomlctl/src returned nothing
- **Evidence-grade**: high
- **Finding**: A visual round exists in the store only as the tasks.toml `changes` reason prefix. No run row, gate round or other record carries it or the verdicts, so the acceptance must carry `round` itself.
- **Details**: The `visual:<id>` `[[run]]` row has `step`, `workflow`, `args_hash`, `run_id`, `outcome` and no round. Gate rounds are stored in `[[gate_round]]`, but visual.md:47 forbids recording the pass there. tomlctl has no code that counts `visual <id> round ` reasons; the orchestrator counts them through `items list --where-prefix`.
- **Counter**: A tomlctl function that parses the `visual <id> round` reason, or a `round` field on `Run`, would let the acceptance be derived rather than stored. Neither exists at HEAD.
- **Impact on plan / report**: The record keys on `(milestone, round)`, with the round passed by the orchestrator. Any check the verb makes on the round's currency needs new tasks.toml reading code.

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:438 (step 10 entry) and visual.md:182-188
- **Source**: drain.md:438, 483, 487; SKILL.md:58, 61, 118; tomlctl/src/tasks/milestones.rs:188-191
- **Evidence-grade**: high
- **Finding**: The commit's visual precondition is prose only. Proceeding on a waiver means editing drain.md step 10, its "Stamping on re-entry" and "Next", SKILL.md rows 58, 61 and 118, and visual.md "Two rounds at most" and "Re-entry". No tomlctl gate exists to relax. `needs-plan`.
- **Details**: The schedule marks the uncommitted milestone `unstamped`. Re-entry then re-runs the gate and the visual pass (drain.md:483), which fails at the same round and stops again unless the pass consults the acceptance. The planner must decide whether an acceptance at the current round skips the re-entry pass, or re-runs it and passes failures confined to waived states and looks.
- **Counter**: If a verb such as `plan stage` or `tasks update --commit` refused to commit or stamp a milestone with `ui_states` lacking a pass, a tomlctl reader would also need changing. No such check exists at HEAD.
- **Impact on plan / report**: One prose task over visual.md, drain.md and SKILL.md, ordered after the verb lands.

- **Library/API or file**: tomlctl/src/plan/schema.rs:35-49 (schema versions) and 233-251 `PlanStore`
- **Source**: schema.rs:1-19 module doc ("Unknown keys are dropped ... every write re-emits it from this shape"), schema.rs:546-559 `write_version`, 593 version refusal
- **Evidence-grade**: high
- **Finding**: A new `[[visual_accept]]` array must be added to `PlanStore`, `from_toml`, `to_toml`, every `PlanStore` literal and a new `write_version` tier. Otherwise any later plan write drops it, and an older binary reading it would lose it without a version refusal.
- **Details**: The store is re-emitted from its typed shape, so unknown keys vanish on the next write. Each prior record kind bumped the stamp: run records gave 3, decision class 4, requests 5. `PlanStore` literals sit at schema.rs:528-542 (the `Default` impl) and in tests at schema.rs:1623, 1722 and 1792, and at schema_request.rs:395. The root-key test lists at schema.rs:1826 and 1845 enumerate the arrays.
- **Counter**: Storing the acceptance as a `[[run]]` skip row (`plan run skip --step visual:<id> --reason ...`) needs no schema change. It loses the structured states, looks and backlog id the brief asks for, and a skip row reads as "not run".
- **Impact on plan / report**: A tomlctl schema task, a leaf task (`PlanRunOp`, dispatch, feature string `cli/types/mod.rs:210-230`, carrier pre-flight `claude/commands/implement-v2.md:29`, `tomlctl/tests/output_options.rs` case, the `tomlctl` skill's `references/plan.md`), and tests in `tomlctl/tests/plan_run_records.rs`.

- **Library/API or file**: tomlctl/src/plan/schema.rs:197-212 `ReportKind`; claude/skills/implement-v2-engine/references/report.md:28-33, 76, 85
- **Source**: run_report.rs:18, 79; schema.rs:1367 (`vocab` on read); glimpse/src/model/plan.rs:160-162, glimpse/src/surface.rs:862
- **Evidence-grade**: high
- **Finding**: To make the report print the waiver, add a `gate:` line template and its `needs_person = false` rule to report.md, and reuse kind `gate`. No tomlctl report code changes, since the report is free text recorded by the orchestrator.
- **Details**: The visual lines already go under `gate` (report.md:76). A new `ReportKind` variant would make a pre-change binary fail reading the store at `vocab(entry, "kind")` instead of the clean version refusal, unless `write_version` also tiers on it. glimpse renders the latest report with `kind` as a string and lists entries whose `needs_person` is true, so an informational waiver line needs `needs_person = false` to stay off the needs-you list.
- **Counter**: If the waiver should sit in glimpse's needs-you list until its backlog item is triaged, then `needs_person = true` is the right value instead.
- **Impact on plan / report**: A report.md edit only. A dedicated kind is optional and costs a schema-tier consideration.

- **Library/API or file**: tomlctl/src/plan/emit.rs:219-226 (`gate-list` `visual.looks` `{task, accept, behaviour, states}`)
- **Source**: emit.rs:170-257; visual.md:17, 153-164
- **Evidence-grade**: high
- **Finding**: The verb can identify and validate the waived criteria by the same `(task, accept)` keys `gate-list` prints. Visual fix tasks carry their look entry in `spec.accept`, so post-round criteria are covered too.
- **Details**: Validation available from the tasks store: each state is in the milestone's `ui_states` and is not a control, and each `(task, accept)` is a `run_by = "look"` entry on a row whose `checkpoint` is the milestone. That the waived states actually failed is unverifiable by the binary, since no verdict is stored.
- **Counter**: If waivers are meant to name free-text criteria or non-look defects (for example a defect class with no look entry), then `looks` alone does not cover them and `defects` must carry them.
- **Impact on plan / report**: The record shape is `states`, `looks` and `defects`. The verb validates the first two.

Line: visual.md fixes the cap at "two rounds", but it is ambiguous whether round 2 means the second capture or the capture after fix round 2. `r` is the minted count plus one (visual.md:31), so the stop falls on pass `r = 3`. Backlog B-799bfc99 reads it as the second pass. Recording the explicit `round` sidesteps this, but the prose that fixes the cap is a sibling concern.

Coverage: read in full visual.md, report.md and run_report.rs, and drain.md steps 6-11. Read schema.rs 1-80, 180-560, 593-662 and 1360-1378; cli/types/plan.rs 125-151 and 425-480; emit.rs 160-257; glimpse model/plan.rs 120-240; the brief; and the B-22eca7ef evidence note, which was empty beyond its title. Checks run: searches for `run visual`, `--accept`, `waive`, round-reason parsing, `.gate_rounds` and `.reports` readers, `unstamped`, and the capabilities feature list. Ruled out:
- A decision row as the vehicle: `Decision` has no milestone, round or state fields, and `binds` takes only tasks or paths.
- A `[[run]]` skip row: it is unstructured and reads as "not run".
- A tomlctl commit gate to relax: none exists.

```toml sources
read_at = "1502e6b420be81caac7bc44ae0e369dc20e461c9"
dirty = ["docs/flows/implement-v2-run-hardening/brief.md"]

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 28
claim = "visual rounds are counted from tasks.toml changes reasons with prefix `visual <id> round `"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 47
claim = "the visual pass is recorded by its run row and notes only, never through plan run gate"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 182
claim = "after round 2 the pass mints nothing and leaves the milestone uncommitted"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 196
claim = "re-entry runs the pass again under the same round"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 438
claim = "the commit runs only once the visual pass has passed"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 483
claim = "re-entry on an unstamped milestone re-runs the gate and then the visual pass"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 487
claim = "a visual pass failed after round 2 reaches the report without a commit"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 61
claim = "commit skip condition includes a visual pass not passed"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 118
claim = "unstamped milestones rerun the gate and visual pass"

[[source]]
path = "claude/skills/implement-v2-engine/references/report.md"
line = 76
claim = "visual pass lines are recorded under report kind gate"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 344
claim = "Run carries no round or verdict field"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 199
claim = "ReportKind vocabulary"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 546
claim = "write_version tiers the schema stamp on which record arrays are present"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 1367
claim = "report entry kind is read through the vocabulary, so an unknown kind errors"

[[source]]
path = "tomlctl/src/plan/run_report.rs"
line = 79
claim = "plan run report validates kind against ReportKind and stores free text"

[[source]]
path = "tomlctl/src/cli/types/plan.rs"
line = 133
claim = "PlanRunOp leaves where a visual leaf would register"

[[source]]
path = "tomlctl/src/plan/dispatch.rs"
line = 31
claim = "plan run dispatch arms"

[[source]]
path = "tomlctl/src/cli/types/mod.rs"
line = 218
claim = "capabilities feature list holding plan_run_records"

[[source]]
path = "claude/commands/implement-v2.md"
line = 29
claim = "carrier pre-flight refuses a binary missing listed features"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 219
claim = "gate-list prints look entries as {task, accept, behaviour, states}"

[[source]]
path = "tomlctl/src/tasks/milestones.rs"
line = 188
claim = "an unstamped milestone empties the schedule pick"

[[source]]
path = "glimpse/src/model/plan.rs"
line = 160
claim = "glimpse renders the latest report with kind as a string"

[[source]]
path = "glimpse/src/surface.rs"
line = 862
claim = "glimpse's needs-you list filters report entries on needs_person"

[[source]]
path = "docs/flows/implement-v2-run-hardening/brief.md"
claim = "success criterion: one command records the acceptance, commit proceeds, report prints it, one backlog item captured"
```
