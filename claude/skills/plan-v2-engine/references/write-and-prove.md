# Design, write, prove

The four steps between Surface and Review: Design records the decisions, contracts and milestones; Surface again puts to the user anything Design raised; Write lays down every task with its spec in one pass; Prove checks the store, cold-reads the milestones and probes the checks. `<slug>` is the flow and `<scratch>` the session scratchpad, which lies outside the repo, so a Write there never prompts. Payloads for `tomlctl` are staged with the Write tool as `.toml` record files, one `[[record]]` per row; no heredocs and no pipes.

Every record below lives in `.claude/flows/<slug>/` and is written only through its verb. `docs/flows/<slug>/design/*.md` is the one thing these steps author with Write, and a write there needs the `Edit(/docs/flows/**)` allow rule or a prompt, as for the brief.

## 1. Design

Design turns the brief, the notes and the source you read into records. The orchestrator owns every one of them; nothing here is delegated.

**Directions.** Design opens by setting the flow's direction with the user. From the brief, the notes and the structure challenge, draft two or three distinct directions the work could take, each with its trade-offs: what it costs, what it risks, and what it makes easy or hard later. Directions differ in approach, not in wording; two that would produce the same tasks are one. Scale the account with the work's complexity and ambiguity: a narrow fix may need a line per direction, a new surface or a cross-cutting change a paragraph each. Ask the user through `AskUserQuestion` to choose one or steer, the direction you would take first with one line of why. This question is Design's own and is not counted against Surface's budget; a question the answer opens is asked in a further round, never assumed. Record the choice as a decision of class `architecture`, `surfaced = true`, the other directions in `rejected`, and apply the answer as the table in [open-and-challenge.md, section 5](open-and-challenge.md#5-surface) says.

Then write `docs/flows/<slug>/design/direction.md` with the Write tool: an `## Overarching` section stating the chosen direction, what it optimises for and what it gives up, and the expectations every task must stay consistent with; and, once Milestones below are cut, one `## Milestone <id>` section per milestone saying what that milestone contributes to the direction and how its tasks should read it. These sections are what every task cites in `spec.design` (section 3), so a worker receives the flow's vision rather than building a generic standalone implementation of its outcome.

**Read-back.** Before any other record, give the user the second read-back that [open-and-challenge.md, section 1](open-and-challenge.md#1-open) promises: the account of what they will see and do once the work lands, made concrete by the investigation and the chosen direction, naming the actual screens, commands, outputs and values they will meet, with every point that changed since the first read-back marked as changed. Ask through `AskUserQuestion` whether it still matches what they mean, and fold every correction into the brief, and into `direction.md` when it touches the direction, before recording anything else.

**Decisions.** Each choice the tasks will rest on is a `plan decide` row: the structure challenge's choose-or-veto, each Unknown a note settled, each option weighed and dropped. Phrase `decision` as an instruction, give `rejected` the fact that sank each alternative, and set `door` to `one-way` when undoing it later would cost more than the plan. `provenance` is `agent-assumed` unless the user said or confirmed it. Every decision carries a `class`, one of the vocabulary [open-and-challenge.md, section 5](open-and-challenge.md#5-surface) names, so veto rates are counted per class.

```toml
[[record]]
decision = "Put the retry loop in its own module and call it from fetch."
why = "fetch is called from four places and only one of them wants retries."
provenance = "agent-assumed"
door = "two-way"
class = "architecture"
binds = ["src/a.rs"]
rejected = [{ option = "Retry inside fetch", because = "the three batch callers already retry and would double their attempts" }]
evidence = "read src/a.rs and every caller the sweep found"
if_false = "the retry moves into fetch and the batch callers drop their own"
revisit_if = "a second caller wants retries"
```

```bash
tomlctl plan decide --slug <slug> --ndjson <scratch>/decide.toml
```

`binds` may name only paths that exist yet, or paths a row's `files` will claim once Write lands; `plan check` reports `plan/decision-bind-unknown` for anything else. Task ids are added after Write (section 3).

**Contracts.** A claim that spans two tasks, such as one producing a shape the other consumes, is a `[[contract]]`, checked at the join rather than inside either task. Give it a `check` that exits non-zero when the claim fails, and its `tasks` once they exist:

```toml
[[record]]
kind = "interface"
claim = "retry::guard takes the request and returns the final response or the last error."
check = "grep -q 'pub fn guard' src/retry.rs"
provides = "retry::guard"
consumes = "fetch"
```

```bash
tomlctl plan contract --slug <slug> --ndjson <scratch>/contract.toml
```

**Milestones.** Every task belongs to a milestone, and Write cannot name one that does not exist yet, so add them here. A gate is a command that must pass at the milestone's drain, such as the touched crate's test suite.

```bash
tomlctl plan milestone add --slug <slug> --id A --goal '<what the milestone delivers>' --gate '<command>'
```

**How many.** `/implement-v2` launches one milestone at a time and stops at each boundary for a drain, a gate and two commits, so a row waits for its milestone even when everything it needs is done. Default to one milestone. Cut another only where the plan wants a gate that must pass before later work starts, a point to replan from what earlier rows produce, or a deferred milestone. Do not cut by theme: rows about different subjects that need nothing from each other belong in one milestone, where the pool runs them together. A row goes in the earliest milestone its `needs` allow. With `--milestones few` the plan has one milestone, and a second only where one must be deferred; say at the gate when a wanted gate or replan point was given up for it.

**Milestone order.** Milestones run in any order their dependencies allow, store order only breaking ties. When one milestone must run after another that none of its rows needs, such as a risk-first order, a handover or install that must land first, or a change to the build environment, add `--after <id>` to the later milestone's `milestone add`. A deferred milestone is stored after every milestone whose output its elaboration may read, and gets `--after <id>` on each such milestone none of its rows already needs: after approval its new rows may need only milestones stored before it, no verb reorders milestones, and the schedule reports it ready to elaborate only from its recorded edges.

A gate that can fail for reasons of this environment, such as a link or install script, gets a probe: add `--gate-probe '<gate> :: <probe>'`, the probe being a cheap command such as the gate's own dry run. Prove runs it (`design/shapes.md#gate-probes`).

Upfront elaboration is the norm. Defer a later milestone only when its detail depends on what earlier ones will produce, and say why with `--defer --reason '<why>'`; its tasks then carry outcome, behaviour, constraints and estimated files, and gain `check`, premises and precedent when it is elaborated.

**UI milestones.** Once the milestones are cut, and before any row is written, a milestone that adds a UI surface or reshapes the layout of one goes through [ui-layout.md](ui-layout.md): you judge which milestones are UI and list the call for every milestone so the user can correct it, the user picks among concrete layouts, and the pick is recorded as a `look` decision and a `design/ui-<milestone>.md` file. Its capture states and controls are declared with `--ui-state` and `--ui-control`, on `plan milestone add` when the step runs before the milestone is added, otherwise on `plan milestone update`. A deferred UI milestone runs the step when it is elaborated.

**Design files.** Write `docs/flows/<slug>/design/<name>.md` only where a task needs prose no record holds: a data shape, a lock order, a migration sequence. `direction.md` and each `ui-<milestone>.md` are design files of this kind, written by the steps above. A task cites a section as `design/<name>.md#<anchor>`, the anchor being the heading slugged by the task store's ref rule, so `## Lock order` is `#lock-order`. `plan check` reports a citation that resolves to no section as `plan/design-anchor-dead`.

**Handovers.** A handover is recorded only for a step no agent can perform, one of the six reasons of `design/handover.md#reasons`. Record one step per row, with `reason`, `action`, `when`, `done_when` and `source_task`. Stage the rows with Write once the tasks exist, since `source_task` names a row, then load them:

```toml
[[record]]
reason = "restart"
action = "Restart the glimpse pane so it loads the rebuilt binary."
when = "After the milestone commit."
done_when = "The pane shows the new version string."
source_task = 3
```

```bash
tomlctl plan handover set --slug <slug> --ndjson <scratch>/handover.toml
```

Landing work an agent can perform goes to its home (`design/handover.md#where-agent-performable-steps-go-instead`): an install or link is a gate on the last milestone. No handover is the expected case.

For a milestone that declares `ui_states`, record no handover asking the user to review the look or to reproduce a visual defect: the milestone's captures and its inspector settle those. State what the user would have checked as a look entry on the row that builds it (section 3).

## 2. Surface again

Design often decides things the first Surface never saw: a dependency it settled on, a scope it widened, a one-way door, a behaviour or a look the user will meet. Put each candidate through Surface's rule in [open-and-challenge.md, section 5](open-and-challenge.md#5-surface): its three tests, its budget, which is per planning session and so counts the questions Surface already asked, and its follow-up rule, under which a question an answer opens is asked in a further round and never assumed. A survivor takes the item shape and `class` that section gives, is asked through `AskUserQuestion` before Write, and has its answer applied by that section's table. A candidate that did not reach the user and still shapes the plan is recorded as that section says for one, `surfaced = false` with its `class`.

## 3. Write

Write every task in one continuous pass, into one record file, landed with one call. The rows are the plan a worker receives, so write them for a reader with none of this session's context.

**Before writing**, confirm the store has no rows:

```bash
tomlctl tasks list --slug <slug> --select id --lines
```

An empty result means the batch's rows are numbered 1, 2, 3 in file order, so `needs`, `coupling` and a premise's `after:<id>` can name a row later in the same file. When rows already exist, this is a revision, not a first Write: change existing rows with `tasks update --ndjson`, add new ones with `needs` only on ids that already exist, and add edges between new rows afterwards with a `set.needs` update naming the `rows[].id` the add printed. An id once issued is never reused, even after its row is removed, so never predict one on a store that has had rows.

**A row** carries the v1 fields a worker and the scheduler read (`title`, `effort`, `files`, `needs`, `checkpoint`, `action`, `detail`) and the `spec` that `plan view --for worker` inlines. `file_notes` annotates a path as `(new)` or `(delete)`, and `closes` and `refs` link backlog ids. The spec's keys and their accepted values are in the `tomlctl` skill's `references/tasks-write.md`, under `tasks add-many`; an `accept` entry's `positive`, `writes_build` and `probe` keys are in its `references/tasks-store.md`.

```toml
[[record]]
title = "Add the retry guard"
effort = "S"
files = ["src/retry.rs", "tests/retry.rs"]
checkpoint = "A"
action = "Wait for the Retry-After delay before retrying a 429."
[record.spec]
outcome = "A 429 response is retried once, after the delay its Retry-After header names."
constraints = ["The public signature of fetch stays as it is.", "Touch no file outside this task's files."]
design = ["design/direction.md#milestone-a"]
precedent = ["src/backoff.rs"]
executor = "lite"
[[record.spec.accept]]
behaviour = "the retry guard lives in its own module"
check = "test -f src/retry.rs"
kind = "forward"
writes_build = false
[[record.spec.accept]]
behaviour = "a caller that gets a 429 receives the response retried after the header's delay"
check = "cargo nextest run -E 'test(retry_after)'"
kind = "forward"
[[record.spec.premises]]
claim = "fn parse exists in src/a.rs"
check = "grep -q 'fn parse' src/a.rs"
at = "base"
[record.file_notes]
"src/retry.rs" = "(new)"
"tests/retry.rs" = "(new)"

[[record]]
title = "Call the retry guard from fetch"
effort = "S"
files = ["src/a.rs"]
needs = [1]
checkpoint = "A"
[record.spec]
outcome = "fetch goes through the retry guard."
design = ["design/direction.md#milestone-a"]
executor = "lite"
[[record.spec.accept]]
behaviour = "fetch calls the guard"
check = "grep -q 'retry::guard(' src/a.rs"
kind = "forward"
writes_build = false
[[record.spec.means]]
text = "Call retry::guard, not a local loop."
binding = "required"
detail = "A local loop would bypass the backoff cap the guard enforces."
```

```bash
tomlctl tasks add-many --slug <slug> --ndjson <scratch>/tasks.toml
```

In this example the `test -f` and `grep` entries prove internal facts, where the code sits and what calls what. The one behaviour a user of `fetch` meets, the retry after a 429, is accepted by the test that drives it, never by a read of the source.

The batch is all-or-nothing: an unknown key, a dangling edge or a cycle lands nothing, and the refusal names the record.

**The spec rules.**

- `outcome` is what must be true afterwards, in the brief's terms.
- `design` cites, on every task, the section of `design/direction.md` that applies to it: its milestone's section, or `#overarching` where the milestone has none of its own. A UI row also cites its surface's section of `design/ui-<milestone>.md`. The worker view inlines each cited section, which is how the flow's direction reaches the worker.
- `behaviour` is yours. It states what must be observably true, and nobody else may change what it asserts: a cold reader may turn it into a `check`, never rewrite it, because a model left to write an oracle captures the code's actual behaviour rather than the intended one.
- `check` must exit non-zero when the behaviour does not hold, the contract a `success:` key has. `kind` is `forward` (fails now, passes after the work), `falsifier` (passes now, naming a check that binds what the task changes) or `guard` (passes now and after, beside a row that discriminates). `positive` is an optional positive-control command, left out when there is none.
- A behaviour the user sees or meets is never accepted on a read-only check alone, since a `grep` or `test -f` proves where code is, not what it does. Give it a check that drives it, such as a test that runs the command or the view, or, in a milestone that declares `ui_states`, a look entry: `run_by = "look"`, `states` naming states from the milestone's `ui_states` and never a control, the `behaviour` written as the criterion the inspector judges, and an empty `check`. Read-only checks are for internal facts, such as a symbol's location or a registration line.

```toml
[[record.spec.accept]]
behaviour = "the section list keeps its width and the form takes the rest, with no dead band below a short form"
run_by = "look"
states = ["settings-empty", "settings-many", "settings-short-window"]
```

- `writes_build = false` is what gets probed. Mark it on every check that only reads, such as `grep`, `test -f`, `git diff` or a `tomlctl` read verb. Leave it out on a check that builds, tests or writes anything, which then runs only at the milestone's gate; absent counts as true.
- `run_by` is set only where the default is wrong (`design/shapes.md#accept-owner`). `gate` marks a read-only check that only the gate can pass. `covered`, with `covered_by` naming the exact gate command, marks a narrow test the milestone's suite gate already runs, so it is not run twice; a worker's own narrow run of such a test is named in `detail`.
- `means` is empty by default. Name a means only when a worker left free would go wrong, and give every `required` entry its reason in `detail`, since wrong or over-specified instructions do more harm than missing ones. A `tool` or `clerk` task gets its targets enumerated, because it will not generalise an instruction from one item to the next; a `deep` task gets a scope clause in `constraints`, because it can expand scope.
- `premises` are the code facts the task stands on, each with a `check` that exits 0 while the fact holds. `at` is `base` for a fact true now, or `after:<id>` for one an earlier task makes true.
- A row whose `files` match an `input` in `.claude/generated.toml` holds each output that generator writes, and its `detail` names the regenerate command, unless a dependent row holds the output and regenerates.
- `executor` is `tool`, `clerk`, `lite` or `deep`. Choose `lite` when the spec states the edit itself and it stays inside one or two files: a doc section, a table row, a flag threaded through, a test for behaviour already written. Choose `deep` when the worker must decide something the spec leaves open, reason across files, or touch security-sensitive code. `deep` is not the safe default: it costs several times the time and context of `lite` on the same task, and a spec too loose for `lite` is usually a spec to tighten.
- A file where every new verb, type or test case is registered, such as a dispatch table or a `mod.rs` of re-exports, is claimed by every task that adds one. Say so in `files` rather than leaving it out to keep tasks disjoint: a task that cannot reach its registration point fails or bends its design to avoid it. Order those tasks with `needs` and keep the per-task work in files only that task holds. A new `tomlctl review` verb puts its clap arguments and its `Verb` impl in its own `tomlctl/src/review/<verb>.rs` and registers in `tomlctl/src/review/mod.rs` alone: the `mod` line, the `ReviewOp` variant whose doc comment is the verb's help, and a one-line `dispatch` arm. A `review run` leaf registers in `tomlctl/src/review/runs.rs` instead. Outside the group, the `FEATURES` list in `tomlctl/src/cli/types/mod.rs` is claimed by a task that adds a capability, together with the three places a test pins to it or that read it: `tomlctl/tests/capabilities.rs`, the sample block and feature table in `tomlctl/README.md`, and the pre-flight list in `claude/commands/implement-v2.md` when the engine depends on the feature; and `tomlctl/tests/output_options.rs` by every task that adds a leaf verb.

**Retitling a row.** A row's `ref` derives from its title, so `tasks update` refuses a new `title` alone. Give the new ref as a row key beside `id`, never inside `set`, and update every design anchor and finding target that names the old ref:

```toml
[[record]]
id = 7
ref = "call-the-retry-guard-from-fetch"
reason = "the task now covers fetch only"
[record.set]
title = "Call the retry guard from fetch"
```

`file_notes`, `closes`, `refs` and `reason` are row keys in the same way.

**Then bind.** Add the new task ids to each decision's `binds` and each contract's `tasks`, with rows naming the printed ids:

```toml
[[record]]
id = "D1"
binds = ["src/a.rs", "2"]
```

```bash
tomlctl plan decide --slug <slug> --ndjson <scratch>/bind.toml
```

A `binds` entry that is all digits names a task, so a decision bound to a task is injected into that task's worker view.

## 4. Prove

Prove runs in this order: the store check, the premises, the cold read and its fixes, and the probes last, because any spec edit unprobes the entry it changes.

### 4a. Check the store

```bash
tomlctl plan check --slug <slug>
```

It exits 1 on any error-class finding. `plan stage` names the verb that clears each class; fix every error and re-run until none is left. A `plan/probe-absent` warning is expected here and is cleared in 4d. A `plan/file-only-chain` info finding names `needs` links that exist only because two rows edit one file; where its longest chain is most of the milestone, split that file so the rows can run together, or keep the chain and move on. A `plan/milestone-width` info finding names rows that could start with an earlier milestone; move each to the milestone its `detail` names with `tasks update --ndjson`, or merge the two milestones, unless the boundary is one the `How many` rule calls for, and with `--milestones few` clear every one. A deferred milestone raises none. A `plan/milestone-order-unqualified` warning names milestones that could run before an earlier one; add `--after` where the order matters, otherwise leave it. A moved row that shares a file with a row of its new milestone needs that row, or the other way round, so the two never run together. A `plan/generated-output-unheld` warning names a row that holds a generator input without all its outputs; add the output to the row's `files`, or confirm a dependent row holds it. A `plan/file-case-collision` warning names a planned file whose name without its last extension differs from a neighbour's only by letter case; rename it so the two differ by more than case, unless the collision is intended and the project has no rule against it.

**Renamed and removed names.** Design can rename or remove a name Surface never swept, and `plan check` cannot see that. For each symbol, key, verb or flag a task's `outcome` renames, removes or reshapes, run the Scope sweep of [open-and-challenge.md, section 5](open-and-challenge.md#5-surface) on the old name now, against the rows as written. Every file with a hit must be in the `files` of a task whose outcome covers that site, tests and documentation included; a hit the plan means to leave is named in that task's `constraints`. A hit in a file no row holds is a missing file or a missing task: fix it with `tasks update --ndjson` or `tasks add-many` before the cold read, because a cold reader looks only at the files its milestone lists and a worker may not leave its own.

### 4b. Premises

Emit a script that runs every `base` premise's `check` of each pending task:

```bash
tomlctl plan probes --slug <slug> --emit --premises > <scratch>/premises.sh
```

Read the script before running it, for the reason 4d gives: each `p` line runs a stored command, and `bash <file>` puts none of them in front of a permission prompt, so this read is the review. Then run it under a timeout:

```bash
timeout 540 bash <scratch>/premises.sh > <scratch>/premises.out
```

Each row of the capture reads `<task>:<premise index>`, `premise`, `sha256=<digest>`, `rc=<code>`, stdout and stderr, tab-separated. `tomlctl plan probes --record <capture> --premises` stores a verdict per premise; read each row's `rc` yourself as well. `rc=0` means the premise holds, and any other code is a false premise. A false premise goes to replanning, never to a stronger executor: rewrite the task, and any task built on it, against what the code holds. When the outcome itself rested on it, that touches intent and goes to the user. `after:<id>` premises cannot hold yet and are not run.

### 4c. Cold read

One fresh agent per upfront milestone reads that milestone's tasks as a worker would and reports what it could not act on, false premises and exact check commands. Each writes `docs/flows/<slug>/notes/cold-read-<milestone>.md`.

**Skip** with a reason for a plan of a few deep tasks, where a worker's own reading covers the same ground, or on `--depth quick`:

```bash
tomlctl plan run skip --slug <slug> --step cold-read --reason '<why>'
```

**Which milestones.** Every milestone whose `elaboration` is not `deferred`, with the pending tasks it holds:

```bash
tomlctl items list .claude/flows/<slug>/tasks.toml --array milestones --select id,elaboration --lines
tomlctl tasks list --slug <slug> --where checkpoint=<milestone> --where status=pending --select id --lines
```

A deferred milestone is read when it is elaborated, not now: it is cold-read then, by references/elaborate.md. A milestone with no pending task is left out.

**Launch.** Delete `docs/flows/<slug>/notes/cold-read-<milestone>.md` for each milestone passed, because a cold reader writes nothing over an existing file and must not read its predecessor. Write the args object to `<scratch>/prove-args.json`; a relaunch needs those exact args, and the hash recorded below is how it knows they are unchanged. Take a fresh fan-out snapshot, as [open-and-challenge.md, section 3](open-and-challenge.md#3-the-fan-out-check) describes, since cold readers hold Write for their note as investigators do: `fanout-check.sh snapshot <scratch>/fanout-cold-read <slug>` with the `docs/flows/<slug>/notes/cold-read-<milestone>.md` path of each milestone this launch asks for, never a snapshot from an earlier launch. Then call:

```
Workflow({name: "plan-v2-prove", args: {slug: "<slug>", milestones: [{id: "A", tasks: [1, 2]}, {id: "B", tasks: [3]}]}})
```

A task id may sit in one milestone only, and the Workflow refuses the whole launch, starting no agent, when one repeats. A result carrying `error` stops the step: report it, and do not fall back to the Agent tool, which this step never uses. Otherwise, in the same turn, record the launch with the args file's hash and end the turn:

```bash
hash=$(sha256sum <scratch>/prove-args.json | cut -d' ' -f1)
tomlctl plan run start --slug <slug> --step cold-read --workflow plan-v2-prove --args-hash "$hash" --run-id <runId> --session "$CLAUDE_CODE_SESSION_ID"
```

**On the notification**, read `<result>` and `<failures>`, then close the run by how it ended:

| The run | Outcome |
|---|---|
| completed, `errors` empty | `ok` |
| completed, `errors` not empty | `errors` |
| failed or killed | `stopped` |

```bash
tomlctl plan run finish --slug <slug> --step cold-read --outcome <outcome>
```

**An index showing no agent launched**, `reads` empty and `errors` ending `no agent launched`, is the Workflow refusing its args. It is a completed run, not a launch `error`: close it as `errors`, stop, report `errors`, and never relaunch the same args.

Otherwise run the fan-out check, `fanout-check.sh check` with the snapshot's arguments. The result is an index, `{slug, reads: [{id, status, unactionable, false_premises, note, error}], errors, script_version}`, and the notes are the answer. Read each entry by its `status`:

| `status` | Meaning | Do |
|---|---|---|
| `ok` | The note is at the path asked for | Read it |
| `no-note` | The agent wrote nothing, or found a file already at the path | Check the path. A file there that predates the launch was not deleted: delete it and relaunch that milestone. Nothing there: relaunch it once |
| `no-return` | The agent returned nothing | Check the path for a note opening `Verdict:` and read it if present; otherwise relaunch it once |
| `error` | The agent failed, and `error` says how | Read the error, then treat it as `no-return` |
| `mismatch` | The agent named a note at another path | The milestone is unread. Delete that file when it is under `notes/`, and relaunch it once |

When the index holds at least one entry and every one is `no-return`, none carrying an `error`, `plan-cold-reader` is not registered in this session. Stop and name the missing entry, `agents/plan-cold-reader.md` under the Claude config dir, linked from this repo's `claude/agents/`. An entry with an `error` names its own cause; read that instead.

A milestone that fails its one relaunch goes to the gate unread, as an assumption `plan gate` shows. Record it as a decision:

```toml
[[record]]
decision = "Approve milestone <milestone> without a cold read"
why = "<how its cold read failed twice>"
provenance = "agent-assumed"
door = "two-way"
binds = ["<task id>", "<task id>"]
evidence = "<the index entries of both launches>"
if_false = "Milestone <milestone> is unread: a worker may meet an unactionable task or a false premise a cold read would have caught"
surfaced = false
```

```bash
tomlctl plan decide --slug <slug> --ndjson <scratch>/cold-read-unread.toml
```

`binds` names the milestone's pending task ids, as strings.

**Apply the results.** The cold reader never writes the store; you do, through `tasks update --ndjson`. Read every point in every note against the code it cites before acting on it:

- **Unactionable.** Settle what the worker would have guessed: name the file, the symbol or the command in `action`, `detail`, `constraints` or `precedent`. When the missing piece is a choice rather than a fact, it is a decision, recorded with `plan decide` and put through Surface again's tests.
- **False premise.** Replan as in 4b.
- **Check command.** A command the reader ran and reported may become a `check`. Adopt it only when it asserts the stated `behaviour`, and never rewrite the `behaviour` to fit a command.

Anything that touches intent is yours alone and is never taken from a note: the brief's Intent and Success criteria, an `outcome`, a `behaviour`, and a decision the user answered. A change to an outcome or a success criterion goes to the user before it lands.

A `spec` key in an update replaces that field whole and leaves the others, so to change one `accept` entry re-issue the whole `accept` list; entries left unchanged keep their probes.

```toml
[[record]]
id = 2
[record.spec]
[[record.spec.accept]]
behaviour = "fetch calls the guard"
check = "grep -q 'retry::guard(' src/a.rs"
kind = "forward"
writes_build = false
[[record.spec.accept]]
behaviour = "fetch keeps its signature"
check = "grep -q 'pub fn fetch(url: &str)' src/a.rs"
kind = "guard"
writes_build = false
```

```bash
tomlctl tasks update --slug <slug> --ndjson <scratch>/cold-read-fixes.toml
```

A `files` or `needs` change on a v2 flow takes no `--unlock-import-fields`, since there is no import to override. Once the plan is approved the store is audited, and every row that moves a plan field then needs a non-blank `reason`, or the update is refused.

Then re-run `plan check` as in 4a.

**Rerun a milestone's cold read** only when one of its tasks changed after its note in a way the note did not ask for: a new or removed task, a changed `outcome`, `files` or premise. Fixing the points a note raised does not call for one, and neither does a check rewritten in 4d. Delete that milestone's note first, then launch afresh as above with only the milestones that need it.

**Relaunch**, with `resumeFromRunId`, is for an open `cold-read` run on re-entry, and almost never applies. A relaunch returns the cached result of every agent whose prompt is unchanged, and a cold reader's prompt is built from the args alone, so a relaunch after any change to the store, the brief, a design file or a task's files returns notes about the plan as it was. Relaunch with `Workflow({name: "plan-v2-prove", args, resumeFromRunId: "<run_id>"})` only when all of these hold:

- the open row's `session` is this session's `CLAUDE_CODE_SESSION_ID`, and the run is no longer live; a run still live is waited for, or stopped with `TaskStop` and the launch's `taskId`;
- `<scratch>/prove-args.json` still exists and its hash, `sha256sum <scratch>/prove-args.json | cut -d' ' -f1`, equals the open row's `args_hash`, read from `tomlctl plan view --slug <slug> --for digest --select stage_inputs.runs`; a row with an empty `args_hash` is abandoned;
- nothing a prompt points at has changed since launch: `fanout-check.sh check` against that launch's snapshot, with its exact arguments, exits 0, and no `tasks` or `plan` write other than the run's own records has landed since. The check fails on any HEAD move unless the snapshot took `--exclude` paths and every commit since touches only the paths those tolerate, so it is the one HEAD test.

Otherwise abandon the run, keep every cold-read note that opens `Verdict:` and whose milestone has not changed since, delete the rest and launch afresh for those milestones:

```bash
tomlctl plan run abandon --slug <slug> --step cold-read --reason '<why>'
```

### 4d. Probes

Probes run last. They cover pending rows' `accept` entries marked `writes_build = false`, each `check` as the negative control and its `positive`, when set, as the positive control. tomlctl never runs a stored command; you do, with your own rights.

```bash
tomlctl plan probes --slug <slug> --emit > <scratch>/probes.sh
```

Read the script before running it. Each `p` line runs a stored command, and running it through `bash <file>` puts none of them in front of a permission prompt, so this read is the review.

The script sets no `errexit`, because a healthy negative control exits non-zero and every later line must still run, and it has no timeout of its own. Run it under one:

```bash
timeout 540 bash <scratch>/probes.sh > <scratch>/probes.out
tomlctl plan probes --slug <slug> --record <scratch>/probes.out
```

540 seconds stays under the Bash tool's 600-second ceiling. Exit 124 means the limit cut the run short: the last row in the capture names the last control to finish, and the script line after it is the one that hung. `--record` refuses a capture that does not cover every probeable control, so record nothing from a cut run. Make the hung check narrow, or drop its `writes_build = false` so it runs only at the milestone's gate, and emit again. A large plan can be probed a milestone at a time with `--milestone <id>` on both `--emit` and `--record`.

`plan probes --record` stores each entry's verdict and prints `{"ok":true,"recorded":[{"task","accept","verdict","reason"}]}`, where `reason` names the control and the rule an unhealthy entry hit. Act on each verdict:

| Verdict | Means | Do |
|---|---|---|
| `healthy` | The check discriminates, polarity as labelled | Nothing |
| `vacuous` | A forward check passes already | Either the work is already done, a false premise to replan as in 4b, or the check tests nothing: rewrite it to fail until the behaviour holds |
| `unsatisfiable` | A forward check fails and its positive control fails too | The command is wrong. Fix the `check` or the `positive`, never the task |
| `permanently-green` | A falsifier fails today | It cannot see this task's change. Strengthen it, or name a check that binds the changed values |
| `guard-alone` | A guard passes with no discriminating entry beside it | Add a `forward` or `falsifier` entry to the same task |
| `broken` | A breakage marker in stderr, a guard that fails, or an unknown `kind` | Fix the path, the tool or the working directory. A `No such file` for a file the task creates means the path belongs on the row's `files` |

Every fix is a `tasks update --ndjson` re-issuing the task's `accept` list, as in 4c, followed by `--emit`, run and `--record` again. `plan check` reports any recorded verdict but `healthy` as `plan/probe-unhealthy`, an error that blocks approval.

After the accept probes, run the gate probes stored at Design. They cover each milestone's `--gate-probe` entries, and the same rule holds: tomlctl never runs a stored command, you do.

```bash
tomlctl plan probes --slug <slug> --emit --gates > <scratch>/gates.sh
```

Read the script, run it under a limit, then record the capture:

```bash
timeout 300 bash <scratch>/gates.sh > <scratch>/gates.out
tomlctl plan probes --slug <slug> --record <scratch>/gates.out --gates
```

A `fails` or `broken` verdict names a gate that cannot pass in this environment. Fix the environment, change the gate, or record a handover; do not leave it to the milestone's drain to find.

### 4e. Where the plan stands

```bash
tomlctl plan stage --slug <slug>
```

`stage` reads `review` once the probes are recorded, without checking the cold read, so confirm a `cold-read` row that is `finished` or `skipped` before moving on, as the engine's Re-entry section says; follow `next` for anything else. A spec edit after the probes, from any step, sends the plan back to 4d for the entries it changed.
