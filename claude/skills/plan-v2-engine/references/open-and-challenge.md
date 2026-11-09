# Open, investigate, challenge, surface

The four steps before Design. Each one writes to disk before the next starts, so a new session finds where the plan stands from `docs/flows/<slug>/` and the plan's run rows, read as the engine's Re-entry section says; `plan stage` alone does not read the runs. `<slug>` is the flow and `<scratch>` the session scratchpad, which lies outside the repo, so a Write there never prompts. Payloads for `tomlctl` are staged with the Write tool as `.toml` record files; no heredocs and no pipes.

## Writing under `docs/flows/`

The brief, `design/` and the notes are authored with Write and Edit, and attended `default` mode prompts on each such write under `docs/flows/**`; `dontAsk` denies it outright. One allow rule pre-approves them: `Edit(/docs/flows/**)` in the project's `.claude/settings.json` or `.claude/settings.local.json`, or `Edit(docs/flows/**)` in user settings, because a leading `/` there anchors at the Claude config dir rather than the project. An investigator's web fetches, MCP calls and any other Bash it runs need allow rules of their own. The engine adds none of these to settings; it names the rule when a prompt or a denial interrupts a step.

## 1. Open

Pick the slug from the goal: a few kebab-case words naming the outcome, matching `^[a-z0-9][a-z0-9-]{0,63}$`. Confirm nothing holds it; an empty result means the slug is free:

```bash
tomlctl flow list --where slug=<slug> --get slug
```

`plan open` is idempotent on its own flow, so a slug that already belongs to another draft would quietly reuse that draft. Choose another slug rather than share one.

```bash
git branch --show-current
tomlctl plan open --slug <slug> [--from <id>[,<id>...]] --branch <branch> [--scope <glob>]...
```

- `--from` carries the backlog ids the carrier was given, and is omitted with none.
- `--branch` is the current branch, from the line above.
- `--scope` repeats once per path glob the goal itself names, and is omitted when it names none. Do not infer globs from what you expect to touch; Design and the sweep below find that.

Then Edit `docs/flows/<slug>/brief.md`, which `plan open` wrote as a skeleton. Fill its six sections from the discussion: Intent, Non-goals, Scope budget, Success criteria, Background, Unknowns. Intent and Success criteria are what the user wants true, in their terms, and the structure challenge is built from them, so keep design out of both. Unknowns lists the questions the plan cannot be written without; each becomes a lens below, or is marked settled when reading settles it first.

## 2. Investigate

A lens is one Unknown phrased as one question on one line, with an id matching `^[a-z0-9][a-z0-9-]*$`. At most 6 lenses go into one launch. Ids `structure-challenge` and those starting `cold-read-` or `backlog-` name other notes in the same directory, so a lens never takes one.

**Skip** with a reason when there is nothing to investigate: no Unknowns, the backlog evidence `plan open --from` wrote already answers them, or the carrier's `--depth quick`.

```bash
tomlctl plan run skip --slug <slug> --step investigate --reason '<why>'
```

**The lens list is the brief.** Before a launch, write each lens it carries into the brief's Unknowns as one line `- <id>: <question>`, the dependency lenses of section 5 included, and mark an Unknown that reading settled as `- <id>: settled, <what settled it>`. The planned lenses are the Unknown ids not marked settled; nothing else holds them across sessions.

**Pending lenses.** List `docs/flows/<slug>/notes/`. A lens is done when `<id>.md` exists there and its first line opens `Verdict:`. Pending is the planned lenses minus the done ones. Delete any lens note that exists and does not open that way, because an investigator writes nothing over an existing file. Pass only the pending lenses.

**Launch.** Write the args object to `<scratch>/investigate-args.json` first; a relaunch needs those exact args, and the hash recorded below is how it knows they are unchanged (section 6). Take a fresh fan-out snapshot, `fanout-check.sh snapshot <scratch>/fanout-investigate <slug>` with each pending lens's note path (section 3). Then call:

```
Workflow({name: "plan-v2-investigate", args: {slug: "<slug>", lenses: [{id: "<id>", question: "<question>"}]}})
```

A result carrying `error` stops the step: report it and do not fall back to the Agent tool, which this step never uses. Otherwise, in the same turn, record the launch with the args file's hash and end the turn:

```bash
hash=$(sha256sum <scratch>/investigate-args.json | cut -d' ' -f1)
tomlctl plan run start --slug <slug> --step investigate --workflow plan-v2-investigate --args-hash "$hash" --run-id <runId> --session "$CLAUDE_CODE_SESSION_ID"
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

**An index showing no agent launched**, `notes` empty and `errors` ending `no agent launched`, is the Workflow refusing its args. It is a completed run, not a launch `error`: close it as `errors`, stop, report `errors`, and never relaunch the same args.

Otherwise run the fan-out check, `fanout-check.sh check` with the snapshot's arguments (section 3). The result is an index, `{slug, notes: [{id, status, verdict, note, error}], errors, script_version}`, and the notes are the answer. Read each entry by its `status`:

| `status` | Meaning | Do |
|---|---|---|
| `ok` | The note is at the path asked for | Read it |
| `no-note` | The agent wrote nothing, or found a file already at the path | Check the path. A file there that predates the launch was not deleted: delete it and relaunch that lens. Nothing there: relaunch it once |
| `no-return` | The agent returned nothing | Check the path for a note opening `Verdict:` and read it if present; otherwise relaunch it once |
| `error` | The agent failed, and `error` says how | Read the error, then treat it as `no-return` |
| `mismatch` | The agent named a note at another path | The lens is unanswered. Delete that file when it is under `notes/`, and relaunch it once |

When the index holds at least one entry and every one is `no-return`, none carrying an `error`, `plan-investigator` is not registered in this session. Stop and name the missing entry, `agents/plan-investigator.md` under the Claude config dir, linked from this repo's `claude/agents/`. An entry with an `error` names its own cause; read that instead.

Recompute the pending lenses from `notes/` as above, never from the returned statuses alone: a lens is done by its note. The one relaunch is a launch with only the pending lenses, made when an answer still decides something; a lens that fails again stays an Unknown, carried into Design as an `agent-assumed` decision that says so.

Read each note's verdict. Before a decision rests on a fact a note reports, read the source it anchors yourself: a note is where to look, not the evidence.

## 3. The fan-out check

Every agent plan-v2 launches holds Write for its one note: investigators, the challenger and cold readers alike. Nothing fences them: Bash can already write anywhere, and no hook limits a Write to the `note:` path. A prompt injection in fetched content could write elsewhere. This check is what compensates, and it runs around every launch: each `plan-v2-investigate` launch, the challenger's dispatch, and each `plan-v2-prove` launch.

**What it captures.** A snapshot before the launch and a check after it compare five captures of the repo. The config catches a planted `core.fsmonitor`, `core.hooksPath` or diff driver, and is compared first, because `git status` and `git diff` run whatever `core.fsmonitor` or diff driver the config names. HEAD catches a commit, which would hide its own edits from the diff. The status catches a new or deleted path, with `-uall`, because without it a new file inside an untracked directory shows only as that directory. The tracked diff catches an edit to a file already dirty before launch. The untracked digests catch an edit to an untracked file, which until approval includes `brief.md` and every note, so they catch a stray file or an overwritten sibling note under `notes/`.

**What it leaves out.** Only the note paths the launch asks for are excluded, because they are what the run writes; every other note stays in. `.claude/flows/<slug>/` is excluded because `plan run start` and `finish` write `plan.toml` inside the window, so a write there by an agent is a residual this check cannot see. Writes outside the repo, under ignored paths, and inside `.git/` beyond its config and HEAD, such as `.git/hooks/`, stay unchecked.

**Running it.** `<skill-dir>` is this skill's base directory, the one the Skill tool reported when it loaded `plan-v2-engine`. `<dir>` is a directory in `<scratch>` for this step's launch: `<scratch>/fanout-investigate`, `<scratch>/fanout-challenge` or `<scratch>/fanout-cold-read`. The note paths are the ones this launch asks for, repo-relative: `docs/flows/<slug>/notes/<id>.md` per lens, `docs/flows/<slug>/notes/cold-read-<milestone>.md` per milestone, `docs/flows/<slug>/notes/structure-challenge.md` for the challenger. Take a fresh snapshot immediately before every launch, never reusing one from an earlier launch, even one moments before:

```bash
bash <skill-dir>/scripts/fanout-check.sh snapshot <dir> <slug> <note-path>...
```

After the run finishes, check against it with the same arguments:

```bash
bash <skill-dir>/scripts/fanout-check.sh check <dir> <slug> <note-path>...
```

Exit 0 means every capture is identical. Exit 2 means the config changed: the script prints the config diff and runs no other git command, and you run none either. Exit 1 names each capture that changed, printing the changed lines of the status and the untracked digests. Exit 3 is a usage error, such as a slug or a note path the script refuses. Exit 1 or 2 stops the plan: show the user the output and the changed paths, and change nothing yourself. A file that a `mismatch` entry names is such a change too; once the user has seen it, the status table's delete-and-relaunch applies.

`<dir>/head-before.txt` is also the launch's HEAD that a relaunch compares against (section 6).

## 4. Structure challenge

One fresh agent drafts the structure this work would want, from the intent, the expected future state and the code alone. It runs without CLAUDE.md and is told not to read `notes/`, `plan.toml` or `tasks.toml`, though only the prompt enforces that last part. Its value so far is measured as agreement rather than discrimination, so judge each draft on its evidence, not on the step's record.

**Skip** with a reason when there is no structure to challenge, such as a one-file fix, or on `--depth quick`:

```bash
tomlctl plan run skip --slug <slug> --step challenge --reason '<why>'
```

**Dispatch.** Delete `docs/flows/<slug>/notes/structure-challenge.md` if it exists, since the challenger writes nothing over a file. Take a fresh fan-out snapshot, `fanout-check.sh snapshot <scratch>/fanout-challenge <slug> docs/flows/<slug>/notes/structure-challenge.md` (section 3), never the one taken before Investigate, then record the start and dispatch one-shot, with no `name:`:

```bash
git rev-parse --show-toplevel
tomlctl plan run start --slug <slug> --step challenge --session "$CLAUDE_CODE_SESSION_ID"
```

```
Agent({subagent_type: "plan-challenger", description: "structure challenge", prompt: "intent: <Intent, from the brief>\nfuture state: <Success criteria, from the brief>\ncode root: <absolute repo root>\nnote: docs/flows/<slug>/notes/structure-challenge.md"})
```

Distil `intent:` and `future state:` from the brief only. Nothing from the notes, your design or the discussion's proposed solution goes in, because the draft is worth having only if it was made without them. Computed signals, such as the most-changed files under the scope from `git log`, may follow as extra lines.

On the return, close the run: `ok` when it returned `{verdict, note}` with the note on disk opening `Verdict:`, `errors` when it returned without one, `stopped` when the dispatch failed. Then run the fan-out check, `fanout-check.sh check` with the snapshot's arguments.

```bash
tomlctl plan run finish --slug <slug> --step challenge --outcome <outcome>
```

**Choose or veto, never merge.** Read the code behind each claim. Either adopt the drafted structure as the plan's own, or keep the present one; a blend of the two is the one choice not open, because merged variants scored lower. Record the choice as a decision in Design. A `restructure` verdict that you would adopt or reject is a Surface candidate like any other, and goes to the user only when it passes the tests below.

## 5. Surface

Before Design elaborates anything, find the assumptions the user should see. A candidate is anything the plan takes for granted, a dependency choice, a reading of the scope or a large restructure among them. It reaches the user only when it passes all three tests:

1. What would change if the answer were no? It must be a different plan.
2. Can I settle it by reading or running something? Then settle it.
3. Does a recorded user stance already answer it? CLAUDE.md, memory and ADRs count.

Zero survivors is the normal case. At most three go to the user; when more pass, ask about the three whose `if_false` changes the plan most and record the rest as below. When nothing survives, record that, so a new session can see Surface ran:

```bash
tomlctl plan run skip --slug <slug> --step surface --reason '<why nothing survived>'
```

**Dependencies.** A new third-party dependency is settled by tools, not by preference. Launch `plan-v2-investigate` exactly as in section 2, as another `investigate` run, with one lens per candidate, id `dep-<name>`, and a question. Form `<name>` in the id by lowercasing the dependency's name, replacing every character outside `[a-z0-9-]` with `-`, collapsing runs of `-` and trimming them from both ends, so `serde_json` gives `dep-serde-json` and `@scope/pkg` gives `dep-scope-pkg`; one id that fails the lens pattern refuses the whole launch. The question keeps the real name, in the form `Build the dependency card for <name>, needed to <need>, by the Dependency card section of the research-methods skill's references/supply-chain-intel.md.` The card gives existence, version, licence, advisories and transitive cost. A dependency that survives the tests shows the user the need, the options including no dependency, the card's live facts, the cost of removing it later, and a note that a model's preference can reflect its training recency.

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

Once the answers are applied, record that Surface ran:

```bash
tomlctl plan run start --slug <slug> --step surface --session "$CLAUDE_CODE_SESSION_ID"
tomlctl plan run finish --slug <slug> --step surface --outcome ok
```

Surface again, after Design, is in [write-and-prove.md](write-and-prove.md#2-surface-again).

## 6. Open runs on re-entry

`plan stage` lists every open run in `open_runs` and makes `next` read `relaunch step <s> or plan run abandon --step <s> --reason <why>` until each is closed. Find the run's session, run id and `args_hash`:

```bash
tomlctl plan view --slug <slug> --for digest --select stage_inputs.runs
```

**A run still live in this session**, its notification not yet arrived, is not re-entry: wait for the notification, or stop it with `TaskStop` and the launch's `taskId`. Never abandon and relaunch over a live run.

**Relaunch** an `investigate` run with `Workflow({name: "plan-v2-investigate", args, resumeFromRunId: "<run_id>"})` only when all of these hold:

- the open row's `session` is this session's `CLAUDE_CODE_SESSION_ID`;
- `<scratch>/investigate-args.json` still exists and its hash, `sha256sum <scratch>/investigate-args.json | cut -d' ' -f1`, equals the open row's `args_hash`; the runtime does not restore the args and the store holds only that hash, so a row with an empty `args_hash` is abandoned. A changed prompt reruns that agent and every one started after it, so trimming or editing the args buys nothing over a fresh launch;
- nothing a prompt points at has changed since launch: `git rev-parse HEAD` equals `<scratch>/fanout-investigate/head-before.txt` and `fanout-check.sh check` against that launch's snapshot, with its arguments, exits 0. A relaunch returns the cached result of each unchanged prompt even when a file it pointed at has since changed.

Record the relaunch on the open row when it notifies, with `plan run finish --slug <slug> --step investigate --run-id <new runId> --outcome <outcome>`, then recompute the pending lenses from `notes/` as in section 2. An agent the relaunch reran meets its own earlier note and returns `no-note`; its note on disk still counts as done when it opens `Verdict:`.

**Otherwise abandon**, which is the only path for a `challenge` run and for any run from another session:

```bash
tomlctl plan run abandon --slug <slug> --step <step> --reason '<why>'
```

Then recompute what is pending as section 2 defines it, from the brief's Unknowns and `notes/`: a lens or challenge note that opens `Verdict:` stands and is not run again, and any other lens the Unknowns plan has its note, if any, deleted and is launched afresh, with a fresh fan-out snapshot before the launch and the check after it.
