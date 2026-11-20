# Gate a milestone

The drain's step 7: the gate's command list, the `verification` dispatch, recording the run, and the fix rounds. It is entered from [drain.md](drain.md), step 6, when every row of the milestone is `done`, and goes on to [finish.md](finish.md).

`<slug>`, `<id>`, `<scratch>` and `<round>` mean what they mean in [run-milestone.md](run-milestone.md): the flow, the milestone, the session scratchpad, and the launch count of this milestone in the session. Payloads for `tomlctl` are `.toml` record files staged in `<scratch>` with the Write tool, a `[[record]]` table per row; no heredocs, no pipes out of `tomlctl`.

## 7. Gate

Every row of the milestone is `done` by now. Build the gate's command list in one call:

```bash
tomlctl plan emit gate-list --slug <slug> --milestone <id>
```

It prints `{commands: [{command, source}], skipped: [{task, accept, reason}]}`, plus `held: [{command, source}]` when the plan marks any command external and `visual` when the milestone has `ui_states`. `commands` holds the milestone's `gates` in stored order, `source` `gate`, then every accept check whose owner is the gate, in row order and within a row in `accept` order, `source` `accept:<task>:<index>`. A check a worker ran is not there, and neither is a command byte-identical to one already listed, which is `skipped` with reason `duplicate`. An accept check its plan marked as covered by one of the milestone's gates is `skipped` with reason `covered_by <command>`: it is not run, and the gate summary in the closing report names each such check, its task and the gate that covered it. A gate the milestone's `external_gates` lists, or a gate-owned accept check whose `effect` is `external`, is printed under `held` and never also under `commands`, in the order it would have had there.

There is nothing to run only when `commands` and `held` are both empty: go to [finish.md](finish.md), step 8, by way of [visual.md](visual.md) when the milestone has `ui_states`, and say in the closing report that the milestone had no gate.

Otherwise dispatch the `verification` agent once, one-shot, with no `name:` and no `model:`:

```
Agent({subagent_type: "verification", description: "gate milestone <id>", prompt: "<the input below>"})
```

The prompt is each `commands[].command` in the order printed, as `verification.md` reads its input, less any command you hold by judgement below; the `held` rows are never in it. Never reorder, filter or de-duplicate the list by hand: the verb already has. A failing accept check is a gate failure like any other: it mints a fix task and costs a round.

```
commands:
- cargo build --manifest-path tomlctl/Cargo.toml
- cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
  timeout: 1500
  rerun: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -j 1 -- --exact {ids}
run_dir: <scratch>/gate-<id>-<n>-1
transient:
- rust-lld: failed to write output.*[Pp]ermission denied
keep_going: true
```

- Each command is one `- ` item, byte for byte as printed.
- The verb prints no options: each command's `timeout:`, `rerun:` and `transient:` options are yours to add, by the rules below.
- `keep_going: true` is always passed, so one gate run reports every failing command and one fix round can answer all of them.
- `run_dir:` is always passed, on every gate dispatch of this step, the base rerun's and the held commands' included: `<scratch>/gate-<id>-<n>-<k>`, `<k>` counting this round's dispatches from 1, so no two share one. The agent keeps each command's `cN.log`, `cN.exit` and `cN.pid` there, where you can find them if it is stopped.
- **Held commands.** The `held` rows are the commands the plan marked as reaching outside the working tree, and they are always held out of this dispatch: under `keep_going` an install or deploy would run from a tree another command just failed. A command the plan did not mark that still changes something outside the build, such as `cargo install` or a script that links or deploys, is held by your judgement, exactly as a marked one: `held` adds to that judgement and never replaces it. When every block of the first call comes back `pass` or `flaky`, or `commands` was empty, dispatch the held commands in a second `verification` call, the `held` rows in the order printed and then the ones you held in their `commands` order, with no question to the user before it: the plan's approval covers them. When a block of the first call fails, they wait for the gate after the fix round. On Windows a held command that installs over a binary, such as `cargo install --path <crate>` or a link script, fails with `os error 5` while that binary runs. Before the held dispatch, check each such binary with `tasklist //FI "IMAGENAME eq <bin>.exe" //NH`, which exits 0 either way and prints the image name only when it is running. When it is running, ask the user through `AskUserQuestion` to close it, with the options "closed it, install now" and "leave the install as a handover". Never stop it yourself, and dispatch only after the answer. A held block that still fails with `os error 5` or `Access is denied` is settled the same way, then dispatched once more and recorded with `--rerun`.
- `timeout:` (seconds, default 540) goes under a gate the project documents as slow. `rerun:` goes under every test command whose runner accepts a list of test ids, with `{ids}` where they go, in the form [verification.md](../../../agents/verification.md) documents for that runner (nextest, `bun test`, vitest), so a test that timed out under load and passes alone is reported `flaky` rather than minted. The base rerun below uses the same option.
- `transient:` carries the project's known environmental failure patterns, from its `CLAUDE.md`; omit it when there are none.

Never run a gate in this conversation, to corroborate a result or for any other reason. The agent returns one block per command it ran, each with an `outcome:`:

| Outcome | Means |
|---|---|
| `pass` | Passed |
| `flaky` | Failed, then passed on a narrow rerun or a runner retry. **A pass.** Its `TANGENTIAL: flaky-test` lines go through [finish.md](finish.md), step 8 |
| `fail` | Failed; the agent went on to the commands after it |
| `timeout` | Ran past its budget; says nothing about the code |

**Record the gate run** as soon as the agent returns, before you act on it. Write the agent's returned text, unchanged and whole, with the Write tool to `<scratch>/implement-<id>-<n>-gate.txt`, and pass it as `--blocks`; never retype a block into a record file:

```bash
tomlctl plan run gate --slug <slug> --milestone <id> --round <n> --blocks <scratch>/implement-<id>-<n>-gate.txt
```

The verb parses each pass line and each block into one row: a pass stores exit `0`, and a block's full failed ids come from the `<log>.ids` file beside its log when the agent left one, so an id past the twentieth or one holding a space is kept. Text that yields no row is refused with `kind=validation`; the format it reads is in [plan-run.md](../../tomlctl/references/plan-run.md), section "Reading the verification agent's text".

`<n>` is `0` for the milestone's first gate run and the fix round's number after that: the highest `<n>` the round count below prints, or `0` when it prints none. The verb copies the tail of each non-pass command's log into `.claude/flows/<slug>/gates/`, which the flow-store commit of [finish.md](finish.md), step 10, stages; a log it could not read is listed in `unread` and stored as `""`, and never fails the record. A second dispatch within the same drain, for the held commands or after a timeout, records under the same round and passes `--rerun`, which merges into the run just recorded and replaces the commands it reran:

```bash
tomlctl plan run gate --slug <slug> --milestone <id> --round <n> --blocks <scratch>/implement-<id>-<n>-gate-rerun.txt --rerun
```

Every other gate call passes no flag and records a new run of its round, so a gate run again at a later invocation keeps the earlier results. This record counts nothing: the rounds already spent still come from the `gate <id> round` reasons, below.

Every block `pass` or `flaky`, the held commands' included, passes the gate, as does a run whose only failures are routed base failures (Fix rounds, below): go to the commit, by way of the visual pass in [visual.md](visual.md) when the milestone has `ui_states`. That pass may mint fix tasks and come back through run-milestone step 3 and this gate before the commit runs. A `timeout` costs no fix round: dispatch once more with a larger `timeout:` on that command, after the fix round when another block is a `fail`. A second `timeout` mints nothing: go on from [finish.md](finish.md), step 8, without the commit, leave the milestone uncommitted, and report it in `needs_human` with the command and its budget.

**A stopped gate agent** returns no blocks, and a command it launched detached can outlive it and hold the project's resources. List its `run_dir`: each `cN.pid` with no `cN.exit` beside it is a launch still running. Stop that process tree by the pid the file holds, never by image name, using the recipe the Step 1b section of `claude/agents/verification.md` gives for the platform that wrote the file (MSYS or Git Bash, or POSIX with `setsid`); a lone `taskkill` or group kill misses the tree there. Confirm the tree is gone by the same pid; a lock file the project keeps stays the project's. Add one `reconcile` entry to the report naming the round, the commands and the pids killed, then re-dispatch the stopped dispatch under the next `<k>` and record it as that dispatch would have been. A tree that will not die goes in `needs_human`, and the round is not re-dispatched.

### Fix rounds

A `fail` is a drain input. A round is spent only on failures the milestone caused, so first rerun the failures at the milestone's base, then route what also fails there, then mint fix tasks for the rest.

#### Rerun at base

At the gate the milestone's work is uncommitted, so `HEAD` is its base. Run this once per recorded gate run with a `fail`, after any `--rerun` of the same drain, over the `fail` commands of the round's latest run as it then stands, in the order that run lists them; a `timeout` is not among them.

1. Make a detached worktree of `HEAD` in the scratchpad, and prepare it before the rerun as the gate's tree is prepared: link or install the dependencies its commands need (a `node_modules` link, say), and copy in the untracked or ignored env files the gate reads (a `.env`, say), which a worktree does not carry:

   ```bash
   git worktree add --detach <scratch>/base-<id>-<n> HEAD
   ```

2. Dispatch the `verification` agent once more, as above, one `- ` item per failing command in that order: `cd <scratch>/base-<id>-<n> && <rerun>`, where `<rerun>` is the command's `rerun:` template with its failed ids put in for `{ids}`, each in single quotes as verification.md fills them, or the whole command when it has no failed ids, no template, or an id holding `'`. Give each item the `timeout:` and `transient:` it had in the gate, and pass `keep_going: true`. Two kinds of failing command are never rerun at base: a held command (in `held`, or one you held by judgement) and a forward accept check (a `source` of `accept:<task>:<index>` naming an entry with `kind = "forward"`), which fails at base by construction. Leave them out of the dispatch; when that leaves it empty, dispatch and record nothing, and every failure is the milestone's own.
3. Write the agent's returned text with the Write tool to `<scratch>/implement-<id>-<n>-gate-base.txt`, its blocks whole and in order, with a `skip: <command>` line on its own, naming the round's failing command, at the position of each command left out of the dispatch, the form [plan-run.md](../../tomlctl/references/plan-run.md), section "Base reruns", documents. The i-th entry answers the i-th failing command, so the count must match. A whole-command rerun that failed before reaching its tests (a missing dependency or env file, a provisioning error: its tail shows no test run) is not a base result: put a `skip:` line in place of its block, so the command counts as the milestone's own failure. Then record it:

   ```bash
   tomlctl plan run gate --slug <slug> --milestone <id> --round <n> --base --blocks <scratch>/implement-<id>-<n>-gate-base.txt
   ```

4. Remove the worktree: `git worktree remove --force <scratch>/base-<id>-<n>`.

The record prints one `commands` entry per failing command, with `base_checked`, `base_failed_ids`, `base_whole_failed` and `base_failure`. A command's own failures are its `failed_ids` less its `base_failed_ids`, or the whole command when it failed whole and is not a `base_failure`, or every failure of a command left `base_checked = false`.

#### Base failures go to the user

The ids in `base_failed_ids`, and a whole command whose `base_failure` is true, predate the milestone: they mint no round-counted fix. Read the waivers the milestone already holds, and the backlog items earlier rounds captured, first:

```bash
tomlctl items list .claude/flows/<slug>/plan.toml --array gate_accept --where milestone=<id> --where kind=base --select round,commands --lines
tomlctl backlog list --grep 'base failure in <slug> milestone <id>' --select id,summary --lines
```

A base failure either names is already routed: it is never asked about again, in this round or a later one. Put every other one to the user through `AskUserQuestion`, one question per command and at most four per call, naming the command, its base-failing ids and the base rerun's summary, with three options:

- **Fix it here.** Mint its fix rows as below, outside the round count: their `reason` opens `base <id>: ` instead of `gate <id> round <n>: `.
- **Backlog it.** Capture one backlog item for it by [finish.md](finish.md), step 8's check and add, its summary opening `base failure in <slug> milestone <id>: ` and naming the command and ids, with `--flow <slug>`.
- **Waive it.** Capture the backlog item as above, then record a gate acceptance of kind `base`, naming only the base-failing ids, staged with the Write tool as `<scratch>/implement-<id>-<n>-base-accept.json`:

  ```json
  {"kind": "base", "why": "<the user's reason>", "backlog": "<the captured item id>", "commands": [{"command": "<the round's failing command>", "failed_ids": ["<a base-failing id>"]}]}
  ```

  ```bash
  tomlctl plan run gate --slug <slug> --milestone <id> --round <n> --accept --json @<scratch>/implement-<id>-<n>-base-accept.json
  ```

  `failed_ids` is a subset of the command's `base_failed_ids`; it is empty only for a command whose `base_whole_failed` is true. A `base` record does not by itself let the milestone commit.

**Routed base failures at the commit.** A base failure backlogged or waived, here or in an earlier round, stays routed: when it fails again in a later gate run it mints nothing and is treated as passing. The first gate run whose only failures are routed base failures passes the gate: record one acceptance naming them all, of kind `accept`, with `backlog` the captured item (the first, when there are several, the rest named in `why`), by the same verb and a payload staged as above, and go to the commit.

#### Mint the milestone's own failures

Read every failing block's `failed_ids` and `tail` for the milestone's own failures, then mint fix tasks in this milestone through `tasks add-many`, in the row shape of [drain.md](drain.md), step 6, "New tasks", one per separable cause across all of them. A build or compile failure usually explains the failures after it: mint for that cause, not for its echoes.

- `files` are the files the failure points at, whichever task last wrote them.
- `accept` holds the narrowest command that reproduces the failure, usually the failed test by id, with `run_by = "worker"`, and `writes_build = true` when it builds: the worker runs its own reproducer, and the pool runs such rows one at a time. It replaces a gate-owned reproducer; the failing gate command still covers the behaviour.
- `executor` is `lite` when the failure names its fix, and `deep` when finding the cause is part of the work.
- `reason` opens `gate <id> round <n>: `, then the failing command and test ids. That prefix is how a later invocation counts the rounds already spent.
- A platform fact the row's spec states follows the outside-facts rule of [SKILL.md](../SKILL.md), section "Your role".

**Carry the cause forward.** A gate failure is often a rule the plan did not state, such as a new verb needing its case in a table-driven test, or a doc lint over a file kind. A pending row that does the same kind of work, in this milestone or a later one, will fail its own gate the same way. For each cause, list the pending rows that hold the failing file or add the same kind of thing, and give each a `constraints` entry naming the rule and the check that enforces it, through the audited update of [drain.md](drain.md), step 6, with a `reason` opening `carried from gate <id> round <n>: `, which the round count below does not match.

**Check each fix row against the decisions.** Before the relaunch, read each new fix row's worker view:

```bash
tomlctl plan view --slug <slug> --for worker --task <n>
```

Its "Decisions binding this task" section holds every decision bound to the row's files, or to a task holding one of them. When the row's `outcome` or `constraints` depart from a decision shown there, ask the user before the launch, through `AskUserQuestion`, whether to amend the decision or reshape the fix; this replaces parking for fix rows, which [drain.md](drain.md), step 6, does for any other change to a decision. An amendment is one `plan decide` row on the decision's own `id`, staged as a `.toml` record file, with the new `decision`, `provenance = "user-confirmed"`, the user's answer in `evidence`, and `binds` re-issued whole with the fix row's id added (read the stored list first with `tomlctl items list .claude/flows/<slug>/plan.toml --array decision --where id=<decision-id> --select binds --lines`):

```bash
tomlctl plan decide --slug <slug> --ndjson <scratch>/implement-<id>-<n>-decide.toml
```

A reshaped fix is an audited update of [drain.md](drain.md), step 6, that brings the row back inside the decision.

**Gate again.** Relaunch through run-milestone step 3, and when every row is `done`, build the next gate's list against the round just fixed:

```bash
tomlctl plan emit gate-list --slug <slug> --milestone <id> --after-round <n>
```

When it prints `"narrowed": true`, every file of the milestone changed only in whitespace since round `<n>`: gate only its `commands`, the ones round `<n>` did not pass, and name each `carried` command in the gate summary of the closing report as passed in round `<n>`. With `"narrowed": false`, gate the full list as before.

**Two rounds at most.** Count the rounds already minted for this milestone:

```bash
tomlctl items list .claude/flows/<slug>/tasks.toml --array changes --where-prefix "reason=gate <id> round " --select ref,reason --lines
```

The highest `<n>` printed is the rounds spent; a `base <id>: ` row is not counted. The cap is two, plus one for each `extra-round` acceptance the milestone holds, which `tomlctl items list .claude/flows/<slug>/plan.toml --array gate_accept --where milestone=<id> --where kind=extra-round --select round --lines` lists. A gate that fails after the last round the cap allows, on anything but routed base failures, mints nothing until the user settles it.

**Past the last round.** Settle a `timeout` first by the rerun above, since an acceptance is refused while the round's latest run holds one. Then ask through `AskUserQuestion`, naming each failing command, its `failed_ids` and the rounds spent, with three options. The two that record stage a payload with the Write tool as `<scratch>/implement-<id>-<n>-accept.json`, `{"kind", "why", "backlog", "commands": [{"command", "failed_ids"}]}` in the base waiver's shape with `why` the user's words, and record it with `tomlctl plan run gate --slug <slug> --milestone <id> --round <n> --accept --json @<scratch>/implement-<id>-<n>-accept.json`, `<n>` the round of the latest gate run:

- **Accept and commit.** Capture one backlog item of kind `debt` for the failures first, by [finish.md](finish.md), step 8's check and add, taking the id of an open item `backlog check` finds for the same failures; then record kind `accept`, naming every failing command of the round's latest run with its `failed_ids` and that item as `backlog`, and go to the commit ([finish.md](finish.md), step 10). The report prints the acceptance by [report.md](report.md).
- **One more fix round.** Record kind `extra-round` with `backlog` empty, naming the failing commands, then mint the round's fix rows as above: the record raises the cap by one, and the round count reads it.
- **Leave it uncommitted.** Go on from [finish.md](finish.md), step 8, without the commit, and report the milestone in `needs_human` with each failing command, its `failed_ids` and the round count. Its fix rows stay `done` and its edits stay in place for the user.
