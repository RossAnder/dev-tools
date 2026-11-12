# Engine

What the implement engine and the agents change so that a run leaves durable, structured records.

## Milestone order

The milestones run tomlctl first, glimpse second, the agent and engine contracts last.

- The engine's references are read live from this repository while a flow runs, so a drain step added to `drain.md` takes effect in the invocation that edits it. The verbs that step calls must already be in the installed binary. The first milestone's gate installs tomlctl, so they are.
- Worker agents are loaded when a session starts, so workers dispatched by the session that edits their contract still write notes without the block. The note reader accepts those.
- Milestone C's first drain stamps plan.toml above what a glimpse built before milestone B can read, so the flow runs `--milestone B`, then handover H1, then milestone C.
- The carrier's pre-flight list changes only in the last milestone. Changed earlier, a re-invocation after a failed first gate would refuse a binary that the same gate was meant to install.

## Drain changes

Three additions to the drain, in this order relative to its present steps.

**After the lane check (step 3), record the attempts.** One call reads every note of the run and upserts an `[[attempt]]` per task:

```bash
tomlctl plan run notes --slug <slug> --milestone <id>
```

It reads `docs/flows/<slug>/notes/run/task-<n>.md` for each task the milestone's latest `implement:<id>` run dispatched, and prints one row per task: `task`, `status`, `block`, the counts of deviations and tangential entries, whether a handover is proposed, and `problem`, a one-line reason when the note is missing, malformed, or its Status word disagrees with the task row. The drain takes tangential discoveries, deviations and handover proposals from this output and from the recorded attempts, `tomlctl items list .claude/flows/<slug>/plan.toml --array attempt`, in place of grepping notes for `TANGENTIAL:` and `Status:` lines. It still reads a note in full for a task that is not `done`, or that reports a deviation or a handover.

It runs after the lane check because the lane check compares the task store's digest and must see no drain write before it; `plan.toml` is outside that digest, and the order keeps the rule simple.

**At the gate (step 7), record each round.** After the verification agent returns, stage its blocks as rows and record them:

```bash
tomlctl plan run gate --slug <slug> --milestone <id> --round <n> --ndjson <scratch>/implement-<id>-<n>-gate.toml
```

`<n>` is `0` for the milestone's first gate run and the fix round's number after that. Each row is one command: `command`, `outcome`, `exit`, `duration_s`, `summary`, `failed_ids` and `log`, the agent's log path. The verb copies the tail of each non-pass log into `.claude/flows/<slug>/gates/`, which the flow-store commit already stages. A second dispatch of the same drain, for held commands or after a timeout, passes `--rerun`, which merges into the run just recorded and replaces the commands it reran. Every other gate call records a new run of its round, so a gate run again at a later invocation keeps the earlier results.

**Before the report (step 11), store it.** Stage the report's entries, one row per line of the report, and record them. A line that only informs, such as an allowed audit command, a run that was not audited or a check a gate covers, is staged with `needs_person = false`:

```bash
tomlctl plan run report --slug <slug> --milestone <id> --why <why> --ndjson <scratch>/implement-<id>-report.toml
```

An invocation with nothing to report still records a report with no entries, so the latest report always describes the latest invocation.

The attempts and gate rounds land in the flow-store commit. The report is written at step 11, after that commit, so when a milestone commits and step 10's Next rule ends the invocation there, record the report before the flow-store commit instead, so it lands in it too. When a milestone ends without a commit, all three stay uncommitted with the rest of the flow's stores, as those do today.

## Agent contracts

- **Workers.** The shared `plan-worker-procedure` block gains the run-note block of `records.md`. The return object does not change. `TANGENTIAL:` lines move into the block's `[[tangential]]` entries; the line form is no longer written.
- **Research agents.** The shared `plan-v2-note-writer` block gains the sources block of `records.md`, and says how to fill `read_at` and `dirty` with two read-only git commands: `git rev-parse HEAD`, and `git status --porcelain -- <the cited paths>`. The return objects do not change.
- **Verification.** Unchanged. Its log directory lives for the session, which is long enough for the gate verb to copy the tails.
