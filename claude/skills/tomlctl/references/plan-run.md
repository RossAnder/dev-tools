# tomlctl — plan run records reference

The four `plan run` leaves that record what an `/implement-v2` invocation did, in `plan.toml`:
`plan run notes` (one `[[attempt]]` per dispatched task), `plan run gate` (one `[[gate_round]]` per
gate run, or with `--accept` one `[[gate_accept]]` per settled round), `plan run report` (one `[[report]]` per invocation) and `plan run visual` (one
`[[visual_accept]]` per accepted visual round). Each takes the required `--slug`
and the write bundle described in [plan.md](plan.md), which also holds `plan run start`, `finish`,
`abandon` and `skip` and every other `plan` verb. The arrays' keys are
[plan-store.md](plan-store.md)'s. Which drain step reaches for each leaf is owned by the
[`implement-v2-engine`](../../implement-v2-engine/SKILL.md) skill.

## Contents

- [`plan run notes`](#plan-run-notes)
- [`plan run gate`](#plan-run-gate)
- [`plan run report`](#plan-run-report)
- [`plan run visual`](#plan-run-visual)

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
`block = "malformed"`. A check whose `result` is outside `pass`, `fail` and `not-run` does not
make the block malformed: that check is left out, the rest of the block is recorded, and the row's
`problem` names it. None of these fails the command, and each row's `problem` is also stored on its
attempt. A second call over unchanged notes writes the same store.

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

When the recorded run's `outcome` is `fail`, after a `--rerun` merge too, the verb also writes its `tree`: one entry per path in the `files` of every row of the milestone, sorted, whether or not it changed, each with `raw`, the SHA-256 of the file's bytes, and `stripped`, the SHA-256 of those bytes with every ASCII whitespace byte (space, tab, LF, form feed, CR) removed. The files are read from disk under the repository root, with no git call; a path that cannot be read as a file stores `"deleted"` for both. A run whose outcome is not `fail` stores no `tree`. A later [`plan emit gate-list --after-round`](plan-engine.md#plan-emit) compares a round's tree with the files as they stand.

```bash
tomlctl plan run gate --slug <slug> --milestone <id> --round <n> --ndjson <path> [--rerun]
tomlctl plan run gate --slug <slug> --milestone <id> --round <n> --blocks <path> [--rerun]
tomlctl plan run gate --slug <slug> --milestone <id> --round <n> --blocks <path> --base
tomlctl plan run gate --slug <slug> --milestone <id> --round <n> --accept --json @<path>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--milestone` | id | The milestone whose gate ran. | required |
| `--round` | integer | `0` for the milestone's first gate run, the fix round's number after that. | required |
| `--ndjson` | source | One row per command, `-` for stdin or a file path (a leading `@` is accepted, and a `.toml` path is a [record file](write.md)). `command` and `outcome` (`pass`, `flaky`, `fail`, `timeout`) are required; an absent or `null` `exit` is omitted from the store; `duration_s` (whole seconds, a fraction rounded), `summary`, `failed_ids` and `log` default to empty. An unknown key or outcome, a blank `command` or a command named twice refuses the batch before any write, naming the line. | one of `--ndjson` and `--blocks` |
| `--blocks` | source | The `verification` agent's returned text as it was returned, `-` for stdin or a file path; excludes `--ndjson`. Each pass line and each block becomes one of the rows `--ndjson` takes, with the same refusals; see [Reading the verification agent's text](#reading-the-verification-agents-text). | one of `--ndjson` and `--blocks` |
| `--rerun` | flag | Merge into the round's latest run, replacing the commands named by `command`, appending new ones and keeping the rest; refused with `kind=validation` when the round has no run. Without it the call records a new run, `seq` one past the round's highest, so a later gate run never overwrites an earlier one. | off |
| `--base` | flag | The rows or blocks are the rerun at the milestone's base of the round's latest run's failing commands; see [Base reruns](#base-reruns). Excludes `--rerun`. | off |
| `--accept` | flag | Record the user's settling of the round's latest run as a `[[gate_accept]]` instead of a run; takes `--json` and excludes `--ndjson`, `--blocks`, `--rerun` and `--base`. See [Gate acceptances](#gate-acceptances). | off |
| `--json` | source | The acceptance payload, `-` for stdin or a file path (a leading `@` is accepted, and a `.toml` path is a [record file](write.md)). Only with `--accept`, which refuses a call without it. | none |

Prints `{"ok":true,"milestone":"A","round":0,"outcome":"fail","commands":[{"command":"cargo test","outcome":"fail","log":".claude/flows/<slug>/gates/A-r0-s1-c1.log"}],"unread":[]}`: `commands` is the round's stored commands after the merge, `unread` the commands of this call whose log was not read; `--lines` splits on `commands`.

### Base reruns

With `--base`, the call records no run. Its rows, or the blocks of `--blocks`, map by order to the commands of the round's latest run whose `outcome` is `fail`, in the order that run lists them: the i-th row answers the i-th failing command, whatever its own `command` reads, since a rerun narrowed to the failed ids names a different command line. The call writes only those commands' `base_checked`, `base_failed_ids` and `base_whole_failed` (keys in [plan-store.md](plan-store.md)), replacing what an earlier base call wrote; it keeps no log and changes no `outcome`, `at` or `tree`.

A command not rerun at base, a held command or a forward accept check, holds its place with a skip: `skip: <command>` on a line of its own in `--blocks` text, or `{"command": "<command>", "skip": true}` in NDJSON, holding no other key. Its `command` must be the failing command at that position, and the command is left `base_checked = false`. A skip is read only with `--base`: without it a `skip:` line is ordinary text between entries and a `skip` row is refused.

Each answered command is set by its base result:

| Base result | `base_checked` | `base_failed_ids` | `base_whole_failed` |
|---|---|---|---|
| `pass` or `flaky` | `true` | empty | absent |
| `fail` with `failed_ids` | `true` | the command's `failed_ids` also in the base result's, in the command's order; a base id the command did not fail with is ignored | absent |
| `fail` with no `failed_ids` | `true` | the command's own `failed_ids`, empty when it had none | `true` only when the command is one of the milestone's `gates` |
| `timeout`, or a skip | `false` | empty | absent |

A command is a base failure when `base_checked` is true and either its `failed_ids` is non-empty and equals `base_failed_ids` as a set, or `base_whole_failed` is true; it is a partial base failure when `base_failed_ids` is a non-empty proper subset. So a whole command that fails in the round and passes at base is not a base failure, though its empty `base_failed_ids` equals its empty `failed_ids`, and an id-less command outside the gates that fails at base is checked but not a base failure.

Refused with `kind=validation`, writing nothing: a round with no recorded run, naming the call that records it; a count of rows or blocks other than the number of failing commands; and a skip naming a command other than the one at its position.

Prints `{"ok":true,"milestone":"A","round":0,"base":true,"commands":[{"command":"cargo test","base_checked":true,"base_failed_ids":["a::b"],"base_whole_failed":false,"base_failure":true}]}`, one entry per failing command in order, `base_failure` being the rule above; `--lines` splits on `commands`.

### Gate acceptances

With `--accept`, the call records no run. It upserts one `[[gate_accept]]` (keys in [plan-store.md](plan-store.md)) checked against the round's latest run, and writes no other part of `plan.toml`; `at` is the time of the call. An `accept` and an `extra-round` share the key (`milestone`, `round`), so either replaces an earlier one of the two for that round; a `base` record is keyed on (`milestone`, `round`, `kind`) and sits beside them. A store holding an acceptance is stamped `7`.

| Payload key | Meaning | Default |
|---|---|---|
| `kind` | `accept` (the user commits past the red gate), `extra-round` (the user grants one more fix round, raising the milestone's round cap by one per record) or `base` (failures that predate the milestone are waived). | required |
| `why` | Why the user settled the round so; refused when blank. | required |
| `backlog` | The backlog item that follows the waiver up: required and non-blank for `accept` and `base`, and empty or absent for `extra-round`. | `""` |
| `commands` | At least one `{command, failed_ids}`, each command named once. `failed_ids` is an array of strings, empty or absent naming the whole command. | required |

Each `commands` entry is checked against the round's latest run:

- For `accept` and `extra-round`, `command` must be a command of that run whose `outcome` is `fail`, and `failed_ids` a subset of its `failed_ids`. An `accept` must name every failing command of the run; an `extra-round` need not.
- For `base`, `command` must be a base or partial base failure of that run (see [Base reruns](#base-reruns)), and `failed_ids` a non-empty subset of its `base_failed_ids`; the command may be named alone, with no ids, only when its `base_whole_failed` is true. A `base` record need not name every failing command.

Refused with `kind=validation`, writing nothing: a malformed payload or an unknown key, naming the field; a round with no recorded run; a run holding a `timeout` command, which the drain reruns with a larger budget first; and any entry the checks above reject, naming the entry, the command and the stray id.

Prints `{"ok":true,"milestone":"A","round":2,"kind":"accept"}`.

### Reading the verification agent's text

`--blocks` reads the two entry shapes `claude/agents/verification.md` returns, without changing that format.

- **A pass line**, `outcome: pass | command: <command> | duration: <n> | summary: <text> | log: <path>`, with an optional ` | retried: transient` tail that is dropped. `command` runs to the last ` | duration: ` followed by an integer and ` | summary: `, and `summary` to the last ` | log: `, so a command holding a shell pipe and a summary holding ` | ` are both kept whole. The row stores `exit = 0`.
- **A block**, for `fail`, `timeout` and `flaky`: a `command: <command>` line directly followed by a line starting `outcome: `, then `exit:` (`none` means absent), `duration_s:`, `log:`, `summary:` and an optional `failed_ids:`. `retried:`, `rerun:`, `TANGENTIAL:` and `tail:` produce no key, and the raw log lines after `tail:` are skipped. A tail ends at a `command: ` line directly followed by an `outcome: ` line, a line starting `outcome: pass | command: `, a line starting `not_run:`, under `--base` a line starting `skip: `, or the end of the text. A refused-command block, carrying only `command`, `outcome`, `exit: none` and `tail:`, takes the defaults.

`summary: none` is the empty summary in both shapes. The command is stored byte for byte, quotes and backslashes included, so `--rerun` replaces the same command rather than adding a second. Blank lines, fence lines and any other line between entries are skipped; text that yields no row is refused with `kind=validation`.

**Failed ids.** When `<log>.ids` exists beside a block's `log` and lies under the OS temp directory, the same rule the kept tail follows, `failed_ids` is its lines, one id each, so an id holding a space and every id past the twentieth are kept. Otherwise the block's `failed_ids:` value is split on spaces and a trailing `(+<n> more)` is dropped.

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
| `--ndjson` | source | One row per entry, `-` for stdin or a file path (a leading `@` is accepted, and a `.toml` path is a [record file](write.md)). `kind` (`parked`, `failed`, `handover`, `no-worker`, `withheld`, `gate`, `audit`, `out-of-lane`, `store-change`, `reconcile`, `request`) and a non-empty `text` are required; a `request` entry reports a worker's request and how it was ruled (see [plan-request.md](plan-request.md)), and is written with `needs_person` `false` when the orchestrator ruled it without the user; `task` defaults to `0`, naming no task; `path` defaults to `""` and must be repo-relative with no `..`; `needs_person` defaults to `true`, and `false` marks a line that only informs. An unknown key or kind, or a bad value, refuses the batch before any write, naming the line. An empty source records no entries. | none: the report has no entries |

Prints `{"ok":true,"milestone":"A","entries":2}`, `entries` being how many entries the report holds.

## `plan run visual`

Records the user's acceptance of a failed visual round as one `[[visual_accept]]`, keyed on (`milestone`, `round`); it writes no other part of `plan.toml`, and a second call for the same milestone and round replaces that record. `at` is the time of the call; an undeclared milestone is refused as in [`plan emit`](plan-engine.md#plan-emit). The verb cannot know which states failed and does not check it. A store holding an acceptance is stamped `6` (see [plan-store.md](plan-store.md)).

```bash
tomlctl plan run visual --slug <slug> --milestone <id> --round <n> --accept --json @<path>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--milestone` | id | The milestone whose visual pass failed. | required |
| `--round` | integer | The visual round accepted. | required |
| `--accept` | flag | Record the acceptance. The only mode, and required. | required |
| `--json` | source | The payload below, `-` for stdin or `@<path>` for a file (a `.toml` path is a [record file](write.md)). | required |

| Payload key | Meaning | Default |
|---|---|---|
| `why` | Why the user accepted the round; refused when blank. | required |
| `states` | The states accepted, at least one, each one of the milestone's `ui_states` and named once; a `ui_controls` entry is refused, a control not being a state. | required |
| `looks` | Each `{task, accept}`: a look criterion waived, `accept` being the 0-based index `plan emit gate-list` prints in `visual.looks`. It must name a `run_by = "look"` accept entry of a row whose `checkpoint` is the milestone. | `[]` |
| `defects` | Each `{state, class, location, detail, note}`: a reported defect accepted. `state` is required and one of `states`; the other four are strings that may be empty or absent. | `[]` |
| `backlog` | The id of the backlog item that follows the waiver up; refused when blank. | required |

An unknown key, in the payload or in an entry, is refused naming the accepted set. Each refusal above is `kind=validation` and writes nothing.

Prints `{"ok":true,"milestone":"A","round":1}`.
