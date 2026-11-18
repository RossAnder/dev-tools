Status: done — the engine's SKILL.md, drain.md and run-milestone.md now route a UI milestone through visual.md and a running worker's questions through questions.md

```toml run-note
files_touched = [
  "claude/skills/implement-v2-engine/SKILL.md",
  "claude/skills/implement-v2-engine/references/drain.md",
  "claude/skills/implement-v2-engine/references/run-milestone.md",
]

[[tangential]]
kind = "bug"
area = "tomlctl/src/plan/schema.rs"
summary = "cli::dispatch::tests::finding_classes (documented_plan_classes_match_the_source, the_plan_scan_reads_both_construction_sites_and_no_tasks_class) fail: no severity resolves for `class` in plan/schema.rs"
why = "The finding-class scanner cannot parse a new construction shape in the uncommitted schema.rs edits; the milestone gate's dispatch-tests step will fail until the scanner or the raise site is fixed"

[[checks]]
command = "accept: grep chain over SKILL.md, drain.md and run-milestone.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

**SKILL.md**
- The role rule "You ask every user question" now also names the build-time question loop and points to questions.md.
- Rule 3 adds a third Workflow launch under the same rules: `implement-v2-visual`, recorded as `visual:<id>`, by visual.md.
- The step table gains a "Visual pass" row (`visual:<id>`) between Gate and Tangential discoveries. It is skipped when the gate has not passed or the milestone has no `ui_states`. The Commit row's skip condition also covers a visual pass that has not passed.
- The loop paragraph counts the visual pass's fix rounds as well.
- The references list gains visual.md and questions.md.
- The open-run rule names `visual:<id>`, with how to close one and point to visual.md's Re-entry.
- Re-entry now says that an unstamped milestone with `ui_states` runs the visual pass after its regated gate. It also says a row parked on an open question is asked that question, by questions.md.

**drain.md**
- Step 2's `parked` row: a row parked on a worker's question is settled first, by questions.md's "At the drain".
- In step 3's out-of-lane paragraph, `.claude/inputs.toml` and its sidecar point to questions.md, and the capture `out` points to visual.md.
- In step 7, both the no-gate path and "passes the gate: go to the commit" now go by way of visual.md when the milestone has `ui_states`.
- Step 9 has a new paragraph, "The look is not a handover". It refuses a handover asking the user to review the look or reproduce a visual defect when the milestone's captures can settle it.
- Step 10 runs only after the visual pass for a UI milestone. The flow-store commit also stages `.claude/inputs.toml` when the question loop wrote it, by questions.md.
- "Stamping on re-entry" now runs the visual pass again before the commit after a regated pass.
- The Next rule lists visual-pass stops among the ways to reach the report without a commit.
- The report template's `gate:` section gains the two visual lines the detail gives. The section-to-kind table says they use kind `gate`.
- "Parked rows at the next invocation" gains a paragraph for a row parked on an open question or on an answered one, pointing to questions.md.

**run-milestone.md**
- The stage check's open-run list and the stale `in-progress` reset now name `visual:`.
- The clean-tree exemptions gain the capture `out`, pointing to visual.md.
- "What comes back" gains "A question mid-run": a `QUESTION <input id> task <n>` message may arrive while the run is live and is handled by questions.md.

Every added clause points to visual.md or questions.md and does not restate them. All three files stayed CRLF throughout.

## Checks

- The accept grep chain passes.
- The `cli::dispatch::tests` subset passed 54 and failed 2. Both failures are in `finding_classes`, and each panic names only `tomlctl/src/plan/schema.rs`, an uncommitted edit outside this task's files. They are not caused by these markdown edits. That file is not on this run's `siblings:` list, so the edit is most likely an earlier task of this milestone, still uncommitted.
