# tomlctl — plan run records reference

The three `plan run` leaves that record what an `/implement-v2` invocation did, in `plan.toml`:
`plan run notes` (one `[[attempt]]` per dispatched task), `plan run gate` (one `[[gate_round]]` per
gate run) and `plan run report` (one `[[report]]` per invocation). Each takes the required `--slug`
and the write bundle described in [plan.md](plan.md), which also holds `plan run start`, `finish`,
`abandon` and `skip` and every other `plan` verb. The arrays' keys are
[plan-store.md](plan-store.md)'s. Which drain step reaches for each leaf is owned by the
[`implement-v2-engine`](../../implement-v2-engine/SKILL.md) skill.

## Contents

- [`plan run notes`](#plan-run-notes)
- [`plan run gate`](#plan-run-gate)
- [`plan run report`](#plan-run-report)

## `plan run notes`

Reads the run note of every task the milestone's latest `implement:<id>` run dispatched and upserts
one `[[attempt]]` per task, keyed on (`task`, the run's `run_id`); it writes no other part of
`plan.toml`. The run is the last unskipped `[[run]]` row of step `implement:<id>`, and its `tasks`
are the ids read. No such run is `kind=not_found`, and one recording no `tasks` is
`kind=validation`; a milestone the task store does not declare is refused as in
[`plan emit`](plan-engine.md#plan-emit).

Each note is `docs/flows/<slug>/notes/run/task-<n>.md`, parsed for its `Status:` line and its
`toml run-note` block; the attempt's keys are those of [plan-store.md](plan-store.md). A missing
note, or one that resolves outside `docs/flows/<slug>/` once symlinks are resolved, records an
attempt with `block = "absent"` and the task row's status. A note without a block keeps its prose
`TANGENTIAL:` lines, and a malformed block, or a first line that is not a `Status:` line, records
`block = "malformed"`. None of these fails the command. A second call over unchanged notes writes
the same store.

```bash
tomlctl plan run notes --slug <slug> --milestone <id>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--milestone` | id | The milestone whose latest `implement:<id>` run's notes are read. | required |

Prints the milestone, the run id and one row per dispatched task, in the run's order; `--lines`
splits on `rows`:

```json
{"ok":true,"milestone":"A","run_id":"wf_1","rows":[{"task":1,"status":"done","block":"present","deviations":1,"tangential":0,"handover":false,"problem":""}]}
```

| Key | Meaning |
|---|---|
| `task` | The task id. |
| `status` | The note's Status word, or the task row's status when the note is missing or has no `Status:` line. |
| `block` | `present`, `absent` or `malformed`. |
| `deviations` | How many deviations the block lists. |
| `tangential` | How many tangential discoveries the block, or a blockless note's prose, lists. |
| `handover` | Whether the block proposes a handover. |
| `problem` | `""`, or one line: the note is missing, the block is malformed and why, the task has no row, or the Status word differs from the task row's status. |

## `plan run gate`

Records one run of a milestone's gate as a `[[gate_round]]` keyed on (`milestone`, `round`, `seq`), from one row per command; it runs nothing and writes only that array and files under `.claude/flows/<slug>/gates/`.
`at` is the time of the call; the round's `outcome` is `pass` when every command is `pass` or `flaky`, `timeout` when every other one timed out, and `fail` otherwise; an undeclared milestone is refused as in [`plan emit`](plan-engine.md#plan-emit).

For a row whose outcome is not `pass`, the verb keeps the last 200 lines of its `log`, cut to the last 64 KiB, as `.claude/flows/<slug>/gates/<milestone>-r<round>-s<seq>-c<n>.log`, `n` being the command's 1-based position in the round after the merge, and stores that repo-relative path as `log`; a `pass` row, or one with no `log`, stores `""`.
A relative `log` resolves against the current directory. The verb reads a log only when its real path lies under the OS temp directory, where the verification agent's `mktemp -d` puts it, since the kept tail is committed with the flow's stores; a log outside it, or one that cannot be read, stores `""` and is listed in `unread`, and never fails the record.

```bash
tomlctl plan run gate --slug <slug> --milestone <id> --round <n> --ndjson <path> [--rerun]
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--milestone` | id | The milestone whose gate ran. | required |
| `--round` | integer | `0` for the milestone's first gate run, the fix round's number after that. | required |
| `--ndjson` | source | One row per command, `-` for stdin or a file path (a leading `@` is accepted, and a `.toml` path is a [record file](write.md)). `command` and `outcome` (`pass`, `flaky`, `fail`, `timeout`) are required; an absent or `null` `exit` is omitted from the store; `duration_s` (whole seconds, a fraction rounded), `summary`, `failed_ids` and `log` default to empty. An unknown key or outcome, a blank `command` or a command named twice refuses the batch before any write, naming the line. | required |
| `--rerun` | flag | Merge into the round's latest run, replacing the commands named by `command`, appending new ones and keeping the rest; refused with `kind=validation` when the round has no run. Without it the call records a new run, `seq` one past the round's highest, so a later gate run never overwrites an earlier one. | off |

Prints `{"ok":true,"milestone":"A","round":0,"outcome":"fail","commands":[{"command":"cargo test","outcome":"fail","log":".claude/flows/<slug>/gates/A-r0-s1-c1.log"}],"unread":[]}`: `commands` is the round's stored commands after the merge, `unread` the commands of this call whose log was not read; `--lines` splits on `commands`.

## `plan run report`

Appends one `[[report]]`, what an orchestrator invocation left for a person, with one entry per row; it writes no other part of `plan.toml` and never replaces or removes an earlier report, so the latest report describes the latest invocation.
`at` is the time of the call; an undeclared milestone is refused as in [`plan emit`](plan-engine.md#plan-emit).

```bash
tomlctl plan run report --slug <slug> --milestone <id> --why <text> [--ndjson <path>]
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--milestone` | id | The milestone the invocation ran. | required |
| `--why` | text | Why the invocation stopped where it did. | required |
| `--ndjson` | source | One row per entry, `-` for stdin or a file path (a leading `@` is accepted, and a `.toml` path is a [record file](write.md)). `kind` (`parked`, `failed`, `handover`, `no-worker`, `withheld`, `gate`, `audit`, `out-of-lane`, `store-change`, `reconcile`) and a non-empty `text` are required; `task` defaults to `0`, naming no task; `path` defaults to `""` and must be repo-relative with no `..`; `needs_person` defaults to `true`, and `false` marks a line that only informs. An unknown key or kind, or a bad value, refuses the batch before any write, naming the line. An empty source records no entries. | none: the report has no entries |

Prints `{"ok":true,"milestone":"A","entries":2}`, `entries` being how many entries the report holds.
