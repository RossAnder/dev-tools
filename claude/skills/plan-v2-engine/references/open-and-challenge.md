# Open, investigate, challenge, surface

The four steps before Design. Each one writes to disk before the next starts, so a new session finds where the plan stands from `docs/flows/<slug>/` and `plan stage` alone. `<slug>` is the flow and `<scratch>` the session scratchpad, which lies outside the repo, so a Write there never prompts. Payloads for `tomlctl` are staged with the Write tool as `.toml` record files; no heredocs and no pipes.

## Writing under `docs/flows/`

The brief, `design/` and the notes are authored with Write and Edit, and attended `default` mode prompts on each such write under `docs/flows/**`; `dontAsk` denies it outright. One allow rule pre-approves them: `Edit(/docs/flows/**)` in the project's `.claude/settings.json` or `.claude/settings.local.json`, or `Edit(docs/flows/**)` in user settings, because a leading `/` there anchors at the Claude config dir rather than the project. An investigator's web fetches, MCP calls and any other Bash it runs need allow rules of their own. The engine adds none of these to settings; it names the rule when a prompt or a denial interrupts a step.

## 1. Open

Pick the slug from the goal: a few kebab-case words naming the outcome, matching `^[a-z0-9][a-z0-9-]{0,63}$`. Confirm nothing holds it; an empty result means the slug is free:

```bash
tomlctl flow list --where slug=<slug> --select slug,status --lines
```

`plan open` is idempotent on its own flow, so a slug that already belongs to another draft would quietly reuse that draft. Choose another slug rather than share one.

```bash
git branch --show-current
tomlctl plan open --slug <slug> [--from <id>[,<id>...]] --branch <branch> [--scope <glob>]...
```

- `--from` carries the backlog ids the carrier was given, and is omitted with none.
- `--branch` is the current branch, from the line above.
- `--scope` repeats once per path glob the goal itself names, and is omitted when it names none. Do not infer globs from what you expect to touch; Design and the sweep below find that.

Then Edit `docs/flows/<slug>/brief.md`, which `plan open` wrote as a skeleton. Fill its six sections from the discussion: Intent, Non-goals, Scope budget, Success criteria, Background, Unknowns. Intent and Success criteria are what the user wants true, in their terms, and the structure challenge is built from them, so keep design out of both. Unknowns lists the questions the plan cannot be written without; each becomes a lens below, or is struck when reading settles it first.

## 2. Investigate

A lens is one Unknown phrased as one question on one line, with an id matching `^[a-z0-9][a-z0-9-]*$`. At most 6 lenses go into one launch. Ids `structure-challenge` and those starting `cold-read-` or `backlog-` name other notes in the same directory, so a lens never takes one.

**Skip** with a reason when there is nothing to investigate: no Unknowns, the backlog evidence `plan open --from` wrote already answers them, or the carrier's `--depth quick`.

```bash
tomlctl plan run skip --slug <slug> --step investigate --reason '<why>'
```

**Pending lenses.** List `docs/flows/<slug>/notes/`. A lens is done when `<id>.md` exists there and its first line opens `Verdict:`. Delete any lens note that exists and does not open that way, because an investigator writes nothing over an existing file. Pass only the lenses not done.

**Launch.** Write the args object to `<scratch>/investigate-args.json` first; it is the only copy of the exact args a relaunch needs (section 6). Take the fan-out snapshot (section 3). Then call:

```
Workflow({name: "plan-v2-investigate", args: {slug: "<slug>", lenses: [{id: "<id>", question: "<question>"}]}})
```

A result carrying `error` stops the step: report it and do not fall back to the Agent tool, which this step never uses. Otherwise, in the same turn, record the launch and end the turn:

```bash
tomlctl plan run start --slug <slug> --step investigate --workflow plan-v2-investigate --run-id <runId> --session "$CLAUDE_CODE_SESSION_ID"
```

**On the notification**, read `<result>` and `<failures>`, then close the run by how it ended:

| The run | Outcome |
|---|---|
| completed, `errors` empty | `ok` |
| completed, `errors` not empty | `errors` |
| failed or killed | `stopped` |

```bash
tomlctl plan run finish --slug <slug> --step investigate --outcome <outcome>
```

Every lens `no-return`, with a null verdict, means `plan-investigator` is not registered in this session. Stop and name the missing entry, `agents/plan-investigator.md` under the Claude config dir, linked from this repo's `claude/agents/`. Then run the fan-out check.

The result is an index and the notes are the answer. Recompute the pending lenses from `notes/` as above, never from the returned statuses alone: a lens is done by its note. A `mismatch` names a note at a path other than the one asked for, which the fan-out check settles. For a lens still pending, launch once more with only those lenses when its answer still decides something; one that fails again stays an Unknown, carried into Design as an `agent-assumed` decision that says so.

Read each note's verdict. Before a decision rests on a fact a note reports, read the source it anchors yourself: a note is where to look, not the evidence.

## 3. The fan-out check

Investigators and the challenger hold Write for their one note, and nothing fences them: Bash can already write anywhere, and no hook limits a Write to the `note:` path. A prompt injection in fetched content could write elsewhere. This check is what compensates, and it runs around every launch in this reference.

Before the launch:

```bash
git status --porcelain=v1 -uall -- . ':(exclude)docs/flows/<slug>/notes' ':(exclude).claude/flows/<slug>' > <scratch>/status-before.txt
git diff HEAD --binary -- . ':(exclude)docs/flows/<slug>/notes' ':(exclude).claude/flows/<slug>' > <scratch>/diff-before.txt
git ls-files -z --others --exclude-standard -- . ':(exclude)docs/flows/<slug>/notes' ':(exclude).claude/flows/<slug>' | xargs -0 -r sha256sum > <scratch>/untracked-before.txt
```

After the run finishes, write the same three as `status-after.txt`, `diff-after.txt` and `untracked-after.txt`, then compare:

```bash
cmp <scratch>/status-before.txt <scratch>/status-after.txt
cmp <scratch>/diff-before.txt <scratch>/diff-after.txt
cmp <scratch>/untracked-before.txt <scratch>/untracked-after.txt
```

All three pairs must be identical. The notes are left out because they are what the run writes, and `.claude/flows/<slug>/` because `plan run start` and `finish` write `plan.toml` inside the window; a write there by an agent is the residual this check cannot see. The status catches a new or deleted path; the tracked diff catches an edit to a file already dirty before launch; the untracked digests catch an edit to an untracked file, which until approval includes `brief.md`. Anything else stops the plan: show the user the changed paths and change nothing yourself. `-uall` matters, because without it a new file inside an untracked directory shows only as that directory. Writes outside the repo, and under ignored paths, stay unchecked.

## 4. Structure challenge

One fresh agent drafts the structure this work would want, from the intent, the expected future state and the code alone. It runs without CLAUDE.md and is told not to read `notes/`, `plan.toml` or `tasks.toml`, though only the prompt enforces that last part. Its value so far is measured as agreement rather than discrimination, so judge each draft on its evidence, not on the step's record.

**Skip** with a reason when there is no structure to challenge, such as a one-file fix, or on `--depth quick`:

```bash
tomlctl plan run skip --slug <slug> --step challenge --reason '<why>'
```

**Dispatch.** Delete `docs/flows/<slug>/notes/structure-challenge.md` if it exists, since the challenger writes nothing over a file. Take the fan-out snapshot, record the start, then dispatch one-shot, with no `name:`:

```bash
git rev-parse --show-toplevel
tomlctl plan run start --slug <slug> --step challenge --session "$CLAUDE_CODE_SESSION_ID"
```

```
Agent({subagent_type: "plan-challenger", description: "structure challenge", prompt: "intent: <Intent, from the brief>\nfuture state: <Success criteria, from the brief>\ncode root: <absolute repo root>\nnote: docs/flows/<slug>/notes/structure-challenge.md"})
```

Distil `intent:` and `future state:` from the brief only. Nothing from the notes, your design or the discussion's proposed solution goes in, because the draft is worth having only if it was made without them. Computed signals, such as the most-changed files under the scope from `git log`, may follow as extra lines.

On the return, close the run: `ok` when it returned `{verdict, note}` with the note on disk opening `Verdict:`, `errors` when it returned without one, `stopped` when the dispatch failed. Then run the fan-out check.

```bash
tomlctl plan run finish --slug <slug> --step challenge --outcome <outcome>
```

**Choose or veto, never merge.** Read the code behind each claim. Either adopt the drafted structure as the plan's own, or keep the present one; a blend of the two is the one choice not open, because merged variants scored lower. Record the choice as a decision in Design. A `restructure` verdict that you would adopt or reject is a Surface candidate like any other, and goes to the user only when it passes the tests below.

## 5. Surface

Before Design elaborates anything, find the assumptions the user should see. A candidate is anything the plan takes for granted, a dependency choice, a reading of the scope or a large restructure among them. It reaches the user only when it passes all three tests:

1. What would change if the answer were no? It must be a different plan.
2. Can I settle it by reading or running something? Then settle it.
3. Does a recorded user stance already answer it? CLAUDE.md, memory and ADRs count.

Zero survivors is the normal case. At most three go to the user; when more pass, ask about the three whose `if_false` changes the plan most and record the rest as below.

**Dependencies.** A new third-party dependency is settled by tools, not by preference. Launch `plan-v2-investigate` exactly as in section 2, as another `investigate` run, with one lens per candidate, id `dep-<name>`, and a question of the form `Build the dependency card for <name>, needed to <need>, by the Dependency card section of the research-methods skill's references/supply-chain-intel.md.` The card gives existence, version, licence, advisories and transitive cost. A dependency that survives the tests shows the user the need, the options including no dependency, the card's live facts, the cost of removing it later, and a note that a model's preference can reflect its training recency.

**Scope.** For each name the change renames, removes or changes the shape of, sweep its call sites:

```bash
tomlctl sweep -e '(?-u:\bNAME\b)' --exclude 'docs/flows/**' --lines
```

The header line carries `truncated` and `coverage_complete`. When `truncated` is true, rerun with a higher `--max-hits` before counting anything. When `coverage_complete` is false, a skipped file can hide a site, so the evidence names the skipped counts and the plan claims no more than the sweep saw. A site outside the brief's scope is a candidate only when it leads to materially different work; otherwise it is a file Design adds.

**Each surfaced item** shows the claim, the evidence read, what changes if it is false, and the default. Record it before asking, as a decision with `surfaced = true`:

```toml
[[record]]
decision = "<the default, phrased as an instruction>"
why = "<one line>"
provenance = "agent-assumed"
door = "one-way"
binds = ["<an existing path>"]
evidence = "<what was read>"
if_false = "<what changes>"
surfaced = true
```

```bash
tomlctl plan decide --slug <slug> --ndjson <scratch>/surface.toml
```

`binds` may name only paths that exist yet, since no task does; Write adds the task ids. Then put each item to the user through `AskUserQuestion`, one question per item, the default first among the options. Apply the answer with a row naming the printed id:

| Answer | Row |
|---|---|
| The default | `provenance = "user-confirmed"`, `vetoed = false` |
| Another option, or the user's own | `decision` rewritten to it, `provenance = "user-stated"`, `vetoed = true` |
| None given | Nothing. The decision stays unanswered, `plan approve` refuses it, and it is asked again before approval |

```toml
[[record]]
id = "D1"
provenance = "user-confirmed"
vetoed = false
```

Every candidate that did not reach the user and still shapes the plan becomes a decision with `provenance = "agent-assumed"` and `surfaced = false`, its `evidence` naming what settled it or the stance that answered it. These are disclosed at approval.

**Surface again.** After Design, run anything Design itself raised through the same three tests and the same item shape. It normally finds nothing.

## 6. Open runs on re-entry

`plan stage` lists every open run in `open_runs` and makes `next` read `relaunch step <s> or plan run abandon --step <s> --reason <why>` until each is closed. Find the run's session and run id:

```bash
tomlctl plan view --slug <slug> --for digest --select stage_inputs.runs
```

**A run still live in this session**, its notification not yet arrived, is not re-entry: wait for the notification, or stop it with `TaskStop` and the launch's `taskId`. Never abandon and relaunch over a live run.

**Relaunch** an `investigate` run with `Workflow({name: "plan-v2-investigate", args, resumeFromRunId: "<run_id>"})` only when all of these hold:

- the open row's `session` is this session's `CLAUDE_CODE_SESSION_ID`;
- `<scratch>/investigate-args.json` still holds the args of that launch, passed unchanged, since the runtime does not restore them and the store does not hold them. A changed prompt reruns that agent and every one started after it, so trimming or editing the args buys nothing over a fresh launch;
- nothing a prompt points at has changed since launch: `git rev-parse HEAD` is the launch's HEAD and the fan-out check against the launch's snapshot passes. A relaunch returns the cached result of each unchanged prompt even when a file it pointed at has since changed.

Record the relaunch on the open row when it notifies, with `plan run finish --slug <slug> --step investigate --run-id <new runId> --outcome <outcome>`, then recompute the pending lenses from `notes/` as in section 2. An agent the relaunch reran meets its own earlier note and returns `no-note`; its note on disk still counts as done when it opens `Verdict:`.

**Otherwise abandon**, which is the only path for a `challenge` run and for any run from another session:

```bash
tomlctl plan run abandon --slug <slug> --step <step> --reason '<why>'
```

Then recompute what is pending from `notes/`: a lens or challenge note that opens `Verdict:` stands and is not run again, and anything else is deleted and launched afresh, with the fan-out check around the launch.
