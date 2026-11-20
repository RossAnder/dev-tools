# Finish a milestone: tangential discoveries to the parked re-ask

The drain's steps 8 to 11 and the parked re-ask: tangential discoveries, handovers, the two commits with stamping and the next milestone, the closing report, and the parked rows at the next invocation. It follows [gate.md](gate.md), or [drain.md](drain.md), step 6, when the milestone cannot gate.

`<slug>`, `<id>`, `<scratch>` and `<round>` mean what they mean in [run-milestone.md](run-milestone.md): the flow, the milestone, the session scratchpad, and the launch count of this milestone in the session. Payloads for `tomlctl` are `.toml` record files staged in `<scratch>` with the Write tool, a `[[record]]` table per row; no heredocs, no pipes out of `tomlctl`.

## 8. Tangential discoveries

The candidates are the `tangential` entries of the attempts [drain.md](drain.md), step 3, recorded for this run, each `{kind, area, summary, why}`, and the `TANGENTIAL: <kind> | <area> | <summary> | <why it matters>` lines of the verification blocks ([gate.md](gate.md), step 7), such as a `flaky` block's `TANGENTIAL: flaky-test` line, which are not notes and so are never recorded as attempts; `TANGENTIAL: none` carries nothing. A tangential entry that reports breakage this milestone's own work caused in a file no row holds, such as a test a task's rename broke, is a replan input ([drain.md](drain.md), step 6) and not a candidate. Each other one is a backlog candidate, and the `backlog-capture` skill owns what earns a row. Never mint blind: stage one JSON object per candidate (`summary`, `kind`, `area`) in `<scratch>/implement-<id>-<round>-tangential.ndjson` with the Write tool, then check them against the store in one read:

```bash
tomlctl backlog check --ndjson <scratch>/implement-<id>-<round>-tangential.ndjson --template '{line} {verdict} {summary}'
```

Mint only what the verdict allows: nothing on `duplicate` or `duplicate-id`, nothing on `likely-duplicate` until you have read the named row and found yours a different problem, and on `related` mint with `--related <the candidate id>`:

```bash
tomlctl backlog add --summary "<summary>" --kind <kind> --area <area> --context "<why it matters>" --origin implement-v2 --flow <slug> --auto-base-sha
```

The summary and context are an agent's words, so quote them into the command only when they hold no quote or `$`; otherwise stage the row as a JSON object and pass it with `--json '@<path>'`. A write to `.claude/backlog.toml` goes into the flow-store commit, not the milestone commit.

**Sources outlive the flow.** A worker records each outside value it verified in its note's `[[sources]]`, which [drain.md](drain.md), step 3, stores on its attempt; read them with `tomlctl items list .claude/flows/<slug>/plan.toml --array attempt --where run_id=<run_id> --select task,sources --lines`. Promote each into `docs/references/<topic>.md` with the Write or Edit tool, `<topic>` being the outside service or library the claim is about, lowercased and hyphenated (`cloudflare-workers`, `strapi`, `docker-hub`), one file per topic, opened with a `# <topic>` title when new. Each value is one `##` heading, the claim, followed by its `url` or `query`, its `how_verified`, its `fetched_at` date, its `version` when set, and `from: <slug> task <n>`. Skip a source whose url (or query) and claim that file already holds. You are the only writer of `docs/references/`, which workers, investigators and elaboration read before they search; each file you wrote goes in the flow-store commit (step 10).

## 9. Handovers

A worker proposes a handover in its note and its index entry's `handover`: a step after landing that only a person can perform, with how to tell it is done. Only you write one. Read the proposal in the note and decide whether it is real: a step an agent could perform is not a handover, so it becomes a fix task in this milestone ([drain.md](drain.md), step 6) or, when it is follow-on work, a backlog candidate (step 8). Stage each real one in `<scratch>/implement-<id>-<round>-handover.toml`:

```toml
[[record]]
reason = "restart"
action = "Restart the Claude Code session."
when = "After the milestone commit"
done_when = "`tomlctl --version` in the new session prints the new version"
source_task = 7
```

```bash
tomlctl plan handover set --slug <slug> --ndjson <scratch>/implement-<id>-<round>-handover.toml
```

`reason` is one of `restart`, `permissions`, `attended`, `observe`, `credentials` and `external`, whose meanings are in [plan.md](../../tomlctl/references/plan.md), section "`plan handover set`"; a proposal none of them fits is not human-only. `action`, `when` and `done_when` must each stand without the plan beside them, and `source_task` is the proposing task. `plan check` reports an undone handover missing any of them as an error, which the next invocation's pre-flight stops on.

A third-party console label, CLI flag or other platform fact you write into `action` or `done_when`, from the worker's proposal or your own, follows the outside-facts rule of [SKILL.md](../SKILL.md), section "Your role".

A handover proposed in the note but empty in the index, or the other way round, is still a proposal: the note is the fuller account.

**The look is not a handover** where the milestone's captures can settle it. On a milestone with `ui_states`, a proposal asking the user to review the look of a state, or to reproduce a visual defect, is work the visual pass does: write no handover for it. A state it names that the pass captures is judged there, by [visual.md](visual.md); one the pass does not capture is a look entry or a `ui_states` gap for the replan ([drain.md](drain.md), step 6), not a person's step.

## 10. Commit

Run this when the gate has passed, when the user accepted its last round (a `[[gate_accept]]` row of kind `accept` for the milestone and that round, by [gate.md](gate.md), step 7's "Past the last round"; a `base` row alone never counts), or when the milestone has no gate, and, for a milestone with `ui_states`, once its visual pass has passed or the user accepted its failed last round, by [visual.md](visual.md), section "Accepting a failed last round". An accepted round is a `[[visual_accept]]` row of the plan store for the milestone and that round; the commit proceeds on the record, and the report prints it under `gate:` ([report.md](report.md)). A visual round that failed without an acceptance reaches the report with no commit.

**Stage exactly the milestone's work.** One call lists it, one unquoted path per line: every changed path, deletions included, that the `files` of a row of the milestone name, wherever it lives, under the flow's own directories too. Stage exactly those paths and commit with the same pathspec, so nothing else already in the index rides along:

```bash
tomlctl plan emit commit-paths --slug <slug> --milestone <id> --for work --get paths
git add -- <path> <path>
git commit -m "<message>" -- <path> <path>
```

Never `git add -A`, `git add .` or `commit -a`. A deleted file in the list is staged as a deletion by the same `git add`. The same call without `--get` prints two more lists, read once before staging:

- `unmatched`: row files that match no changed path and no ignore rule. Nothing to stage: the row named a file it left unchanged, or one an earlier commit already holds.
- `ignored`: `{path, rule}` for each row file the repository's ignore rules hide. Stage none of them and never `git add -f`: the ignore rules are the repository's. Put one `reconcile` entry per path in the report ([report.md](report.md)), quoting the rule, with `needs_person = true`, so the user can add an exception or accept that the file stays local.

**The message** comes from the `commit-conventions` skill, applied to the staged tree. Draft it there; never write one directly.

**A failed commit**, a pre-commit hook rejecting it among them, stops the engine: report the hook's output, stamp nothing, and leave the tree as it is.

**Stamp** the sha on every row of the milestone:

```bash
git rev-parse HEAD
tomlctl tasks update <N1,N2> --slug <slug> --commit <sha>
```

A backlog row a milestone task closes may now be complete; reconcile it before the second commit, so its resolution lands there:

```bash
tomlctl backlog reconcile --flow <slug> --apply
```

When the milestone holds an apply row, reconcile its findings here too, by [apply-rows.md](apply-rows.md), section "Reconcile".

**The flow-store commit.** The stores, the run's notes, the gate logs, the backlog and the reference files of step 8 go in a second, small commit of their own, with its message from the `commit-conventions` skill too. When the Next rule below ends the invocation at this milestone, gather and record the report first, by "Record the report" in [report.md](report.md), so the report lands in this commit; when it goes on to the next milestone, the report waits for step 11. Then list its paths in a second call, made now and not before the milestone commit, since the stamp, the reconciles and the report have written stores since:

```bash
tomlctl plan emit commit-paths --slug <slug> --milestone <id> --for flow --get paths
git add -- <each path that printed> <each reference file step 8 wrote>
git commit -m "<message>" -- <each path that printed> <each reference file step 8 wrote>
```

The verb never prints `docs/references/`, so stage each reference file step 8 promoted into explicitly, beside the printed paths, as above. The list holds every changed path under `.claude/flows/<slug>/` and `docs/flows/<slug>/notes/run/`, the backlog store, the inputs store the question loop writes, and each review store a `[[finding_links]]` entry of the milestone names outside the flow's directory, each with its `.sha256` sidecar, which the pre-commit hook compares with its store; a path the milestone commit took is never in it. The same call without `--get` prints `ignored` too: `{path, rule}` for each such path the ignore rules hide, such as a gate log under a `*.log` rule or the backlog under a `.claude/*` rule. Stage none of them and never `git add -f`; put one `reconcile` entry per path in the report, quoting `rule`, with `needs_person = true`, so the user can add an exception or accept that the record stays local. When the report is recorded here, read `ignored` with one call before recording it, so those entries land in it, and take `paths` from a call made after. This commit may stage another session's elaboration writes, which stay non-runnable because that milestone stays deferred until its elaboration's last write.

### Stamping on re-entry

A milestone whose rows are all `done` with an empty `commit` is one whose run ended between the commit and the stamp, or before the gate. When `HEAD` is a milestone commit holding the rows' files, so that `tomlctl plan emit commit-paths --slug <slug> --milestone <id> --for work --get paths` prints nothing and `git -c core.quotePath=false show --name-only --format= HEAD` lists them, stamp `HEAD`'s sha as above, then, when the milestone holds an apply row, run [apply-rows.md](apply-rows.md) **Reconcile** before the flow-store commit, and make the flow-store commit. A milestone whose plan store holds a `[[gate_accept]]` row of kind `accept` for its last gate round, read as the visual acceptance is read (`tomlctl items list .claude/flows/<slug>/plan.toml --array gate_accept --where milestone=<id> --where kind=accept --select round --lines`, against the highest round its `[[gate_round]]` rows hold), is never gated again: it goes on as a gate that passed, to the visual pass or the commit. Otherwise a milestone holding no apply row runs the gate again from [gate.md](gate.md), step 7, and one holding an apply row goes back through the tail of [drain.md](drain.md), step 6, first: the **Reproduce check** when a row fixes a finding, then the **Settle drain** when a row settles one, then the gate. Running the settle drain again is safe: it skips a finding an earlier pass recorded. On a milestone with `ui_states`, a gate that passes here runs the visual pass again before the commit, by [visual.md](visual.md), its round counted from the `visual <id> round ` reasons already minted; when the user accepted that round, its step 2 finds the `[[visual_accept]]` row, captures nothing and goes straight to the commit.

### Next

Go to run-milestone step 2 when this milestone was committed and `--milestone` was not given; step 2's schedule decides whether anything runs, so a parked row no longer stops the engine by itself. Otherwise end the invocation with the report below: the stop rule of [drain.md](drain.md), step 6, a gate the user left uncommitted after its last round, or one that timed out twice, and a visual pass that was unsound, failed after round 2 without the user's acceptance, or could not run ([visual.md](visual.md)), reach here without a commit. A visual pass whose last round the user accepted committed, and goes on like any committed milestone.

## 11. The `needs_human` report

The report, with how it is recorded in the plan store, is in [report.md](report.md).

## Parked rows at the next invocation

The skill's Re-entry runs this before run-milestone step 3, for each milestone the schedule reports `parked` before the pick, as well as for the milestone about to run, and for each parked row of it:

```bash
tomlctl tasks list --slug <slug> --where checkpoint=<id> --where status=parked --select id,title --lines
```

A row parked on a question its worker posted is settled from the question, not the note: one left with an open question on the `plan` ledger is asked that question's own prompt and options, and one whose question is already answered is reset without asking, both by [questions.md](questions.md), section "At the drain", whose bound decision is the record of the answer in place of the constraint below.

Read each other row's note: its `Status:` line, its evidence and the decision it says would unblock it. Ask the user through `AskUserQuestion`, one question per row and at most four per call, with options drawn from the note: unblock it as the note proposes, unblock it another way, drop the task, or leave it parked.

The answer must reach the store as a plan field, because moving `status` back to `pending` writes no `[[changes]]` row and so records no answer. Record it in `spec.constraints`, where the next worker reads it in its view, in the same row that unparks the task, with the answer as the `reason`:

```toml
[[record]]
id = 12
status = "pending"
reason = "user: keep the v1 field name; the rename belongs to a later plan"
[record.spec]
constraints = ["<each constraint the spec already holds>", "Keep the v1 field name `parked_at`"]
```

```bash
tomlctl tasks update --slug <slug> --ndjson <scratch>/implement-<id>-unpark.toml
```

An answer that changes more than a constraint, such as the task's outcome or its files, moves those fields in the same row. A task the user drops goes with `tomlctl tasks remove <n> --slug <slug> --force --reason '<the answer>'`, which re-points its dependents at its own needs; any edit it left in its files is the user's to keep or discard, and the next pre-flight names those files until someone does. A row left parked stays parked; the milestone runs its disjoint work and cannot gate.
