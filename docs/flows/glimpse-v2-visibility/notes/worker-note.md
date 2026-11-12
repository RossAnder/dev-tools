Verdict: The run note's shape is defined once, in the shared `plan-worker-procedure` block (claude/agents/plan-worker-deep.md:60-89, byte-identical in plan-worker-lite.md), and the return object is fixed by `RETURN_SCHEMA` at claude/workflows/implement-v2-milestone.js:37-50. Nothing in tomlctl or glimpse reads a note today; only the drain prose does (drain.md:42-47). A machine-read block needs five changes: it goes after the `Status:` line, not before it; the orchestrator's own parked note and every old note must still parse without one; the task store's whole-store rewrite needs a new `Store` section and a schema bump, because unknown keys are dropped (tomlctl/src/tasks/schema.rs:17-18); the record verb runs after the lane check; and the verb is keyed per attempt, because a relaunched worker replaces the note.

## Answer

**Who defines the note and the return.** Both are defined in the shared `plan-worker-procedure` block. `scripts/shared-blocks.toml:80-83` holds that block byte-identical across the two worker agents. Step 9 writes the note, the "The note" section (deep.md:60-79) gives its shape, and "Return" (deep.md:81-89) gives the object `{task, status, note, files_touched, deviations, handover, needs_files}`. The Workflow sets the note path (`notePath`, implement-v2-milestone.js:153), restates the return in the prompt (:169), validates it against `RETURN_SCHEMA` (:37-50, `additionalProperties: false`) and copies `deviations`, `handover` and `needs_files` into the index entry (`toEntry`, :192-226). It never opens the note. The only check it makes is that the returned path ends with the expected path (`namesNote`, :172).

**Who writes a note.** The worker writes it. The orchestrator also writes a run note when it parks a row itself (drain.md:141), with a `Status: parked —` first line and prose evidence. A relaunched worker reads the earlier note and then replaces it (deep.md:62). So the note on disk only ever holds the latest attempt.

**Who reads it.** Only the orchestrator, following drain prose:
- step 2 decides which notes to read in full from the index's `status`, `deviations` and `handover` (drain.md:42);
- it greps every note's `TANGENTIAL:` lines and its `Status:` line, and checks that line against the index (drain.md:44-47);
- the out-of-lane check uses the notes and their `files_touched` (drain.md:71);
- the replan reads failed notes and the per-path `needs_files` lines (drain.md:125, 172);
- the handover proposal is read from the note (drain.md:354, 371);
- the `needs_human` report quotes the Status line (drain.md:440);
- the parked re-ask reads the note (drain.md:477).

No Rust code reads a note. `plan drift` excludes `docs/flows/<slug>/` by design (tomlctl/src/plan/drift.rs:173). glimpse reads nothing under `docs/flows/` (brief, Background).

**The index is not durable.** It exists only in the Workflow's result (run-milestone.md:233). An abandoned run has no index at all (implement-v2-engine/SKILL.md:107). So today `deviations` and `handover` survive only as prose in the latest note.

**What the block changes:**
1. **Placement.** Keep `Status: <s> — <line>` as line 1 and put the block right after it. Line 1 is read by drain.md:47, 440 and 477, by the orchestrator's own parked note (drain.md:141), and by the settle-task rule "opens `Status:` like any other" (deep.md:50). All 118 run notes on disk conform to it today.
2. **The writer contract.** Edit the shared block once, in both agents. `verify-shared-blocks.sh` enforces byte-identity. The block's fields would be the deviations (what, why, which means), the handover, `needs_files` with their reasons, the tangentials, and the checks run. The `TANGENTIAL:` line format is grepped (drain.md:44, 338), so either keep emitting it beside the block or move that grep to the verb.
3. **The return object.** The return can stay as it is. The drain triage table (drain.md:29-36) and the Workflow's `needs_files` validation (:177-188) both use the return, and the verb can cross-check the return against the block, the way drain.md:47 already checks the Status line. Shrinking the return instead would touch implement-v2-milestone.js:37-50, 169, 190 and 209-214, `workerReturn` in claude/workflows/tests/harness.test.mjs:368-376, drain.md:27 and 31, and run-milestone.md:233.
4. **The task-store section.** The tasks store drops unknown keys on every write (schema.rs:17-18). Every worker's `tasks update` rewrites the whole store through the installed binary. So the section needs:
   - a `Store` field (schema.rs:85-127) and its `from_toml` and `to_toml` arms (:669-780, :782-898);
   - an entry in `holds_v2_keys` (:502-509);
   - a schema stamp above 2 (`SCHEMA_VERSION`, :30; refusal at :681), so that an older binary refuses the store instead of silently deleting the records.

   The implement-v2 carrier's pre-flight also needs a new feature name (claude/commands/implement-v2.md:29; the list is tomlctl/src/cli/types/mod.rs:213, asserted in tomlctl/tests/capabilities.rs:1861).
5. **The lane check.** A new top-level key falls into the digest's `rest` section (tomlctl/src/plan/lane.rs:41-50, 109-112). The drain allows no store write before the lane check except step 4's resets (drain.md:63). So the record verb runs after step 3, or the section gets its own digest group. The digest is retaken at every launch, so records from earlier rounds do not trip it.
6. **The verb.** The closest precedent is `review reproduce`: NDJSON rows with a closed key set, upserted on a round key (claude/skills/tomlctl/references/review-apply.md:122-126). Key the record on task plus run id or round, because the note keeps only the latest attempt. The verb must accept a note with no block (the orchestrator's parked note, and every note from before the contract; the brief rules out backfill) and record "no block" for it, rather than refusing. A new leaf verb needs the dispatch flag-table and `command_lint` gates, a flag table in claude/skills/tomlctl/references/tasks-write.md, and its case in tomlctl/tests/output_options.rs.
7. **Readers.** glimpse gets the records through tomlctl/src/tasks/snapshot.rs (sibling lens snapshot-path). In the drain, the step 2 grep of `TANGENTIAL:` and `Status:` and the step 6 deviation input can read the verb's output instead of the prose.

## Findings

- **Library/API or file**: claude/agents/plan-worker-deep.md:60-89 (`plan-worker-procedure` shared block, "The note" and "Return"); same text in claude/agents/plan-worker-lite.md:60-89
- **Source**: file read; scripts/shared-blocks.toml:80-83 lists both carriers
- **Evidence-grade**: high
- **Finding**: The note and the return contract are defined only here, so the block's writer side is one shared-block edit applied to both agents.
- **Details**: The note opens `Status: <done|failed|parked> — <one line>`, followed by prose sections and `TANGENTIAL:` lines. The return is `{task, status, note, files_touched, deviations, handover, needs_files}`, with `needs_files` capped at 16 repo-relative paths. The settle rule (deep.md:50) relies on the `Status:` opening too.
- **Counter**: Another file restating the note shape would have to change as well. A grep for `opens with .Status` and `notes/run` over claude/, tomlctl/, glimpse/ and scripts/ found only the drain and the Workflow.
- **Impact on plan / report**: The writer task holds both agent files and leaves `Status:` on line 1.

- **Library/API or file**: claude/workflows/implement-v2-milestone.js:37-50 (`RETURN_SCHEMA`), :153 (`notePath`), :169 (`workerPrompt`), :192-226 (`toEntry`)
- **Source**: file read; claude/workflows/tests/harness.test.mjs:368-376 (`workerReturn`)
- **Evidence-grade**: high
- **Finding**: The Workflow validates the return but never reads the note, so a note-side block needs no Workflow change unless the return is shrunk.
- **Details**: `additionalProperties: false` means any new return key needs a schema edit, a prompt edit at :169 and a harness fixture edit. `deviations` and `handover` pass straight into the index entry. No Rust gate covers this script: a grep of tomlctl/ for `implement-v2-milestone` and `RETURN_SCHEMA` found nothing.
- **Counter**: The plan might move deviations into the return instead of the note. Then this file and the harness test become the change, and the index still is not durable.
- **Impact on plan / report**: Prefer keeping the return unchanged and having the verb cross-check it against the block.

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:42-47, 63, 71, 141, 172, 354, 440, 477
- **Source**: file read; claude/skills/implement-v2-engine/SKILL.md:107; run-milestone.md:233
- **Evidence-grade**: high
- **Finding**: The drain is the only reader of notes, the orchestrator is a second note writer (parked rows), and the index is never persisted. So the deviations, handovers and tangentials of earlier attempts are lost when a relaunch replaces the note.
- **Details**: Step 2 greps `TANGENTIAL:` and `Status:`. Step 6 reads the failed notes and the per-path `needs_files` reasons. Steps 9, 11 and the parked re-ask read the note text. An abandoned run has no index, so its notes are the only record.
- **Counter**: A persisted copy of the index elsewhere would make this wrong. Only the transcript path is kept in scratch (run-milestone.md:216), and scratch is session-local.
- **Impact on plan / report**: The record verb belongs in drain step 2 or 3, after the lane check. It needs a "no block" outcome for orchestrator-written and pre-contract notes.

- **Library/API or file**: tomlctl/src/tasks/schema.rs:17-18, :30, :85-127 (`Store`), :502-509 (`holds_v2_keys`), :669-898 (`from_toml` / `to_toml`)
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: A new task-store section would be silently deleted by any binary that predates it, and every worker's `tasks update` rewrites the whole store. So the section needs a schema stamp the old binary refuses, plus a capabilities feature the carrier pre-flight checks.
- **Details**: The module doc says "Unknown keys are dropped — the store is tool-owned and every write re-emits it from this shape." The current refusal is `schema_version > SCHEMA_VERSION` (`2`, :681). The stamp is computed from content (:823-826), so a v3 stamp would be conditional on the new key, as v2 is. The pre-flight feature list lives at tomlctl/src/cli/types/mod.rs:213 and tomlctl/tests/capabilities.rs:1861, and claude/commands/implement-v2.md:29 checks it. needs-plan: schema, stamp, verb and pre-flight must move together.
- **Counter**: Recording into plan.toml's run records (`Run`, tomlctl/src/plan/schema.rs) avoids the worker-rewrite hazard, since workers do not write plan.toml. That alternative is not ruled out here, and its schema was not read.
- **Impact on plan / report**: One task covers the schema, the stamp and the feature together. The plan should decide between the task store and the plan store's runs.

- **Library/API or file**: tomlctl/src/plan/lane.rs:41-50 (`SECTIONS`, `REST`), :109-112
- **Source**: file read; drain.md:63
- **Evidence-grade**: high
- **Finding**: A new top-level key lands in the lane digest's `rest` section. A drain write before the lane check would therefore show up as a `{source: "digest", section: "rest"}` store write.
- **Details**: The digest blanks only the dispatched rows' `status` and `agent`. The drain rule is "Run it before any store write of the drain but step 4's resets". Since `launch-args` retakes the digest at every launch, earlier rounds' records are harmless.
- **Counter**: Placing the record verb after drain step 3 avoids any lane.rs change. Adding a named section to `SECTIONS` is optional, and `the_digest_names_every_section` (:491-495) would then need updating.
- **Impact on plan / report**: The drain step order is a constraint on the drain-edit task.

- **Library/API or file**: docs/flows/*/notes/run/*.md (118 notes across human-only-handovers, milestone-cadence, v2-apply and v2-harness-tuning)
- **Source**: `rg -n "^#+ .*[Dd]eviation.*" docs/flows/*/notes/run` → `58 ## Deviations`, `6 ## Deviations and choices`, `2 ## Deviations / choices`, `1 ## Deviations and gap-filling choices`; a first-line loop → `118 notes, 0 without a conforming first line`
- **Evidence-grade**: high
- **Finding**: Workers spell the deviation heading four ways, while the `Status:` first line is uniform. Parsing the prose would be brittle, and the first line is a stable anchor to keep.
- **Details**: The procedure names no headings, only bullet content (deep.md:62-69). The brief's "Deviations and choices" is the minority spelling.
- **Counter**: If glimpse only needed to render the note as markdown, no block would be needed. The deviations feed and the history across attempts are what need it.
- **Impact on plan / report**: The block should carry deviations as records, and glimpse should not parse headings.

Coverage: read in full: plan-worker-deep.md, implement-v2-milestone.js, drain.md, brief.md, lane.rs:1-128, schema.rs:1-40, 60-150, 495-514 and 669-898. Grep sweeps: `notes/run|run note|files_touched|Deviations and choices` over the tree (lumina hits are its own unrelated `files_touched`); `plan-worker|notes/run` over tomlctl/src, glimpse/src, claude/workflows, the skills and scripts; `implement-v2-milestone|RETURN_SCHEMA` over tomlctl. Checks run: deviation-heading counts and Status first-line conformance over every run note. Ruled out: a Rust or glimpse reader of notes (none exists); a Rust gate over the Workflow schema (none); `plan drift` (excludes the notes by design, drift.rs:173). Not read: plan/schema.rs `Run`, snapshot.rs internals (sibling lens snapshot-path), tasks-write.md flag tables.
