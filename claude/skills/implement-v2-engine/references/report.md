# The needs_human report

The report every invocation ends with, and how it is recorded in the plan store. It is step 11 of [drain.md](drain.md); the commit rules it leans on are in that file.

Every invocation ends with this report, whether it stopped on a parked row, a gate, an error or the last milestone, and records it in the plan store before printing it (Record the report, below). Gather the open items first:

```bash
tomlctl plan view --slug <slug> --for digest
tomlctl tasks list --slug <slug> --where status=parked --select id,checkpoint,title --lines
tomlctl tasks list --slug <slug> --where status=failed --select id,ref,checkpoint --lines
```

`open_items.undone_handovers` lists each handover not yet done as `{id, reason, action, when, done_when, source_task}`; `open_items.parked_tasks` lists the parked ids. Every `failed` row goes in the report, whichever path ended the invocation: the reason relaunching stopped is the stop rule of [drain.md](drain.md), step 6, when that fired, and otherwise the reason the invocation ended. Report in this shape, each section omitted when it is empty:

```
needs_human — <slug>, stopped at milestone <id>: <why the invocation ended>

parked:
- task <n> (<checkpoint>): <the note's Status line> — docs/flows/<slug>/notes/run/task-<n>.md
failed:
- task <n> <ref> (<checkpoint>): <why relaunching stopped> — docs/flows/<slug>/notes/run/task-<n>.md
handovers:
- <handover id> from task <n> (<reason>): <action> — when: <when>; done when: <done_when>
no worker:
- task <n>: executor <tool|clerk>
withheld:
- task <n>: premise <i> <fails|broken> — <the claim>
- task <n>: handover <id> not done — <action>
gate:
- milestone <id> failed after round <n>: <command> — <failed_ids>
- milestone <id> timed out twice: <command> — timeout <seconds>
- milestone <id> gate round <n> accepted: <commands>; backlog <B-id>
- milestone <id> gate round <n>: one more round granted
- milestone <id> base failure: <command> <ids>, <fixed|backlogged|waived>
- milestone <id> base failure: <command> <ids>, waived; backlog <B-id>
- milestone <id> did not run task <n> accept <i>: covered_by <command>
- milestone <id> visual round <n> unsound: control <state> passed
- milestone <id> visual failed after round <n>: <state> - <capture path>
- milestone <id> visual round <r> accepted: <states>; backlog <B-id>
- milestone <id> visual round <r>: inspector disagreement on <states>, decided by the user
audit:
- <agent id>: <command> — <allowed|breach>
- run <run_id> not audited: no transcript directory was kept
out of lane:
- <path> — <the task whose note accounts for it, or "no task">
store changes:
- task <n>: <field> <old> → <new>, written during run <run_id>
reconcile:
- task <n>: <the problem plan run notes printed> — docs/flows/<slug>/notes/run/task-<n>.md
requests:
- task <n> <request-id> <p|c><k> granted|denied|needs-user: <reason>

milestones committed this invocation: <id> <sha>, …
flow: <plan stage>; <next step>
```

The last line names what happens next: `/implement-v2 <slug>` once the parked rows have an answer; the schedule's pick when there is one, and each deferred milestone with `/plan-v2 <slug> --elaborate <id>`; and when every milestone is committed, that the flow's status is still to be moved, which this engine does not do.

### Record the report

Before the report is printed, record it in the flow's plan store, so the latest report always describes the latest invocation. When a milestone committed and [drain.md](drain.md), step 10's Next rule ended the invocation there, this ran before that step's flow-store commit and is not repeated here; otherwise it runs here, and stays uncommitted with the rest of the flow's stores. Stage one row per line of the report's sections in `<scratch>/implement-<id>-report.toml`:

```toml
[[record]]
kind = "parked"
task = 12
text = "task 12 (B): Status: parked — the rename needs a decision"
path = "docs/flows/<slug>/notes/run/task-12.md"

[[record]]
kind = "audit"
text = "a1b2c3: cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests — allowed"
needs_person = false
```

| Section | `kind` |
|---|---|
| `parked:` | `parked` |
| `failed:` | `failed` |
| `handovers:` | `handover` |
| `no worker:` | `no-worker` |
| `withheld:` | `withheld` |
| `gate:` | `gate`, the visual pass's lines included |
| `audit:` | `audit` |
| `out of lane:` | `out-of-lane` |
| `store changes:` | `store-change` |
| `reconcile:` | `reconcile` |
| `requests:` | `request` |

The `requests:` section lists every item the drain's Granted requests step settled ([drain.md](drain.md), step 5a, by [requests.md](requests.md)), one line per item, granted and denied alike. A request a worker ruled or marked applied itself is not listed there: its command goes under `audit:` as a breach, and each path it granted under `out of lane:`.

`text` is the line as printed, without its `- `. `task` is the task the line names, left out when it names none. `path` is the run note or file the line names, repo-relative, left out when it names none; an out-of-lane line's path is the path itself. `needs_person = false` goes on a line that only informs: an `allowed` audit row, a run that was not audited, a check a gate covered (`did not run task <n> accept <i>: covered_by <command>`), a visual round the user accepted (`visual round <r> accepted: <states>; backlog <B-id>`, its `<states>` comma-separated and its waiver followed up by that backlog item), each gate exception the user decided, and every `requests:` line, since the drain has already acted on each ruling. The gate exceptions are a gate round accepted (`gate round <n> accepted: <commands>; backlog <B-id>`, its `<commands>` comma-separated, one line per `[[gate_accept]]` row of kind `accept`), one more round granted (`gate round <n>: one more round granted`, one line per row of kind `extra-round`), a base failure, one line per command of the round's latest run that is a base or partial base failure (`base failure: <command> <ids>, <fixed|backlogged|waived>`, by how the user settled it; a row of kind `base` is reported as `base failure: <command> <ids>, waived; backlog <B-id>`, and never as a gate accepted, since it waives only its ids and commits nothing by itself), and a visual inspector disagreement the user decided (`visual round <r>: inspector disagreement on <states>, decided by the user`). Every other line needs a person and leaves the key out. `--why` is the reason on the report's first line, quoted as one argument:

```bash
tomlctl plan run report --slug <slug> --milestone <id> --why <why> --ndjson <scratch>/implement-<id>-report.toml
```

An invocation with nothing to report still records a report: call it without `--ndjson`, which records one with no entries. The milestone is the one the report's first line names.
