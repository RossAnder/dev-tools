# Elaborate a deferred milestone

Elaboration writes the checks, premises and precedent of a milestone that was deferred at plan time, once the milestones it stands on have built what it needs. It runs in an attended session as `/plan-v2 <slug> --elaborate <id>`, on a flow whose plan is approved or already running, and it may run while `/implement-v2` runs another milestone of the same flow in a second session. `<slug>` is the flow, `<id>` the milestone, and `<scratch>` the session scratchpad, as in [write-and-prove.md](write-and-prove.md). Payloads are staged with the Write tool as `.toml` record files, one `[[record]]` per row.

The steps, in order: preconditions, surface and decide, revise the rows, prove while the milestone is still deferred, flip it to elaborated, then finish the run and commit. The order is the point of the procedure. `/implement-v2` picks a milestone once it is no longer deferred, so the milestone stays deferred through the store check, the premises, the `cold-read:<id>` cold read and the probes, and the flip is the last write to `tasks.toml`. No verb and no Workflow filters on deferral (only `plan/accept-no-check` is relaxed for a deferred milestone's rows), so everything can be proved before the flip, and the flip cannot be undone: no flag moves a milestone back to `deferred`.

The run rows are milestone-scoped: `elaborate:<id>` for the whole procedure and `cold-read:<id>` for its cold read. `/implement-v2` leaves both alone, `plan stage` lists them in `open_runs` without a blocker after approval, and `plan emit schedule` reports a deferred milestone with an open `elaborate:<id>` run as `elaborating`, so another session does not start the same work.

## 1. Preconditions

```bash
tomlctl flow doctor --slug <slug>
tomlctl plan stage --slug <slug>
tomlctl plan emit schedule --slug <slug>
```

`stage` must read `approved` or `running`. An earlier stage means the plan is still being authored: stop, and name `/plan-v2 <slug>` without `--elaborate`. `done` means there is nothing left to elaborate. A `blockers` entry other than an open run, such as a brief changed since approval, stops the procedure with that entry quoted; a changed brief is settled with `plan approve --amend` by [approve.md](approve.md) before elaborating.

Find `<id>` in the schedule's `milestones` and act on its `state`:

| `state` | Do |
|---|---|
| `deferred` | Continue |
| `elaborating` | An `elaborate:<id>` run is open. Read its row (below). This session's own row is a resume, by section 7. Another session's row means that session holds the milestone: ask the user through `AskUserQuestion` whether that session is dead before anything else, and abandon its run only on a yes |
| anything else | Refuse. The milestone is upfront or already elaborated, and its rows are run, not elaborated |

```bash
tomlctl plan view --slug <slug> --for digest --select stage_inputs.runs
```

**An `on` that is not empty** names upstream milestones not yet done and committed. Their code is not built, so a check or premise written now would describe code that does not exist. Put it to the user through `AskUserQuestion`, naming each milestone in `on` and its state, with stopping until they are committed as the default. Continue only on the user's say-so, and write premises that depend on unbuilt code as `after:<task>` rather than `base`.

**Record the run before any write**, so a second session sees the milestone as `elaborating` and a dead session leaves its trace:

```bash
tomlctl plan run start --slug <slug> --step elaborate:<id> --session "$CLAUDE_CODE_SESSION_ID"
```

**Read what the milestone stands on.** The defer reason, the brief, the notes and the milestone's rows as they are:

```bash
tomlctl items list .claude/flows/<slug>/tasks.toml --array milestones --select id,elaboration,defer_reason,gates --lines
tomlctl tasks list --slug <slug> --where checkpoint=<id> --select id,title,status,files,needs --lines
tomlctl plan view --slug <slug> --for worker --task <task>
```

Then read what the upstream milestones actually built, from the code at HEAD, not from their rows' `outcome`: each row's `commit` names the commit that landed it, and `git show --stat <sha>` the files it touched. A deferred milestone was deferred because its detail depended on this, so a row whose estimated `files` or `outcome` the built code no longer fits is revised in section 3, and an `outcome` that no longer holds goes to the user.

## 2. Surface and decide

Put each assumption the elaboration rests on through the three tests of [open-and-challenge.md, section 5](open-and-challenge.md#5-surface). The budget is four questions in the first round, asked in one `AskUserQuestion` call of up to four; when more survive, ask the four whose `if_false` changes the most for the user. A question an answer opens is asked in a further call outside the budget, and rounds go on until no answer opens a new one, never answered by assumption. Each is recorded first as a decision with `surfaced = true` and a `class`, and its answer recorded after. When nothing survives, record nothing.

**A UI milestone.** When the milestone adds a surface or reshapes an arrangement, run the layout step of [ui-layout.md](ui-layout.md) before any row is revised: the user picks among drafted options, the pick is a decision of class `look`, and the pictures and `design/ui-<milestone>.md` are written under `docs/flows/<slug>/design/`. Refine the milestone's section of `design/direction.md` where the pick changes it, and cite that section from each row of the milestone in `spec.design`.

Each choice the revised rows rest on is a `plan decide` row, in the shape of [write-and-prove.md, section 1](write-and-prove.md#1-design), `provenance = "agent-assumed"` unless the user said or confirmed it:

```bash
tomlctl plan decide --slug <slug> --ndjson <scratch>/elaborate-decide.toml
```

Write `docs/flows/<slug>/design/<name>.md` only where a row needs prose no record holds, cited as `design/<name>.md#<anchor>`, by the "Design files" paragraph of the same section. A gate the elaboration changes goes through `plan milestone update` with `--gate`, `--clear-gate` or `--gate-probe` and a `--reason`, since the store is audited.

## 3. Revise the rows

The store has had rows, so this is a revision: never predict an id, change existing rows with `tasks update --ndjson`, and add new rows with `tasks add-many`. Every row needs a non-blank `reason`, because an approved store is audited and refuses a plan-field change without one. The spec's keys and rules are [write-and-prove.md, section 3](write-and-prove.md#3-write), "The spec rules"; elaboration adds what deferral left out, an `accept` entry's `check`, the `premises` and the `precedent`, and tightens `files` against the code section 1 read.

**New rows** go in with `checkpoint = "<id>"` and `needs` only on ids that already exist, in this milestone or in a milestone stored before it in `[[checkpoints]]`:

```bash
tomlctl tasks add-many --slug <slug> --ndjson <scratch>/elaborate-add.toml
```

A row that would need a row of a milestone stored after this one means the plan's milestone order is wrong, and `checkpoint/invalid-cut` would refuse it anyway. Do not write it: stop and put the order to the user, since reordering milestones changes what every later milestone waits on.

**Existing rows** take their plan fields under `[record.set]`, and `spec`, `reason`, `ref` and `file_notes` as row keys. `tasks update --ndjson` refuses `action` or `detail` outside `set`, though `add-many` takes them as row keys. A changed `title` needs its new `ref` beside `id`, by write-and-prove's "Retitling a row". A `spec` key replaces each field it supplies whole and keeps the others, so re-issue the whole `accept` list:

```toml
[[record]]
id = 14
ref = "render-the-colour-field-from-the-kit-tokens"
reason = "elaborated against the token barrel milestone B landed"
[record.set]
title = "Render the colour field from the kit tokens"
action = "Replace the hard-coded swatches in ColourField.tsx with the kit's token list."
detail = "Read the tokens from the kit's public barrel. The swatch order follows the barrel's export order."
checkpoint = "C"
[record.spec]
precedent = ["src/fields/SizeField.tsx"]
[[record.spec.accept]]
behaviour = "the colour field reads the kit's tokens"
check = "grep -q 'KIT_TOKENS' src/fields/ColourField.tsx"
kind = "forward"
writes_build = false
[[record.spec.premises]]
claim = "the kit's public barrel exports KIT_TOKENS"
check = "grep -q 'export.*KIT_TOKENS' packages/kit/src/index.ts"
at = "base"
```

```bash
tomlctl tasks update --slug <slug> --ndjson <scratch>/elaborate-update.toml
```

**Then the edges and binds.** Add `needs` between new rows with a `set.needs` update naming the `rows[].id` that `add-many` printed, each with its `reason`. Add the new ids to each decision's `binds` and each contract's `tasks`, by write-and-prove's "Then bind":

```bash
tomlctl plan decide --slug <slug> --ndjson <scratch>/elaborate-bind.toml
tomlctl plan contract --slug <slug> --ndjson <scratch>/elaborate-contract.toml
```

**Then the renamed-names sweep** of [write-and-prove.md, section 4a](write-and-prove.md#4a-check-the-store), "Renamed and removed names", for every name a revised `outcome` renames, removes or reshapes, against the rows as written now.

## 4. Prove while deferred

The milestone is still `deferred` through every part of this section, so nothing here makes its rows runnable. Run the parts in this order; any spec edit after the probes sends the entries it changed back to 4d.

### 4a. The store check

```bash
tomlctl plan check --slug <slug> --milestone <id>
```

`--milestone` reports the findings on this milestone's rows and its upstream milestones' rows, and the graph, cut and handover classes whatever they name, so another milestone's half-finished state does not refuse this one. Fix every error and re-run until none is left. `plan/accept-no-check` is still relaxed here because the milestone is deferred, so read each row's `accept` list yourself now: an entry with a blank `check` is an error section 5's check will report after the flip.

### 4b. Premises

```bash
tomlctl plan probes --slug <slug> --emit --premises --milestone <id> > <scratch>/elaborate-premises.sh
```

Read the script, run it under a limit and record the capture with the same scope, by [write-and-prove.md, section 4b](write-and-prove.md#4b-premises):

```bash
timeout 540 bash <scratch>/elaborate-premises.sh > <scratch>/elaborate-premises.out
tomlctl plan probes --slug <slug> --record <scratch>/elaborate-premises.out --premises --milestone <id>
```

A false premise is replanned in section 3's terms, never handed to a stronger executor. A premise check is usually a grep, which can hold while the claim it stands for is false, so a clean premise run is not a reason to skip the cold read.

### 4c. The cold read

One `plan-cold-reader` reads this milestone's rows as a worker would, through `plan-v2-prove` with this milestone only. It is skipped only under [write-and-prove.md, section 4c](write-and-prove.md#4c-cold-read)'s rule for a few deep tasks, recorded with `tomlctl plan run skip --slug <slug> --step cold-read:<id> --reason '<why>'`; `--depth quick` does not skip it here, because nothing else reads an elaborated milestone before a worker does.

**Delete** `docs/flows/<slug>/notes/cold-read-<id>.md` if it exists: a cold reader writes nothing over an existing file and must not read its predecessor.

**The args.** Write `{slug: "<slug>", milestones: [{id: "<id>", tasks: [<pending ids>]}]}` to `<scratch>/elaborate-prove-args.json`, the pending ids from:

```bash
tomlctl tasks list --slug <slug> --where checkpoint=<id> --where status=pending --select id --lines
```

**The excludes.** A live `/implement-v2` run in another session edits code, writes run notes and commits while the cold read runs, and the fan-out check would report each of those as a change. Exclude what that session may write: every file in the `files` of every row of each milestone whose schedule `state` is neither `done`, `empty`, `deferred` nor `elaborating`, not only the one running now, since the run may move on to its next milestone during the read; `docs/flows/<slug>/notes/run`; and, when the task store has `[[finding_links]]`, each review store they name and its `.sha256`. Run the schedule again for the states, then read the files and stores:

```bash
tomlctl plan emit schedule --slug <slug>
tomlctl tasks list --slug <slug> --where checkpoint=<milestone> --select files --lines
tomlctl items list .claude/flows/<slug>/tasks.toml --array finding_links --select store --lines
```

Keep the list in `<scratch>/elaborate-excludes.txt`, one path per line with repeats dropped, because the check must pass exactly the paths the snapshot took; the script refuses a check whose `--exclude` paths differ from the snapshot's. With any `--exclude`, the script also tolerates the backlog store and lets HEAD move through commits that touch only excluded paths, the note, `.claude/flows/<slug>/` and `docs/flows/<slug>/notes/run/`, which covers the other session's milestone and flow-store commits. A commit touching anything else still fails the check, which is the injection guard the excludes keep.

**Snapshot, launch, record.** `<skill-dir>` is this skill's base directory, as in [open-and-challenge.md, section 3](open-and-challenge.md#3-the-fan-out-check). Take a fresh snapshot immediately before the launch, every `--exclude` ahead of the note path:

```bash
bash <skill-dir>/scripts/fanout-check.sh snapshot <scratch>/fanout-cold-read-<id> <slug> --exclude <path> --exclude docs/flows/<slug>/notes/run docs/flows/<slug>/notes/cold-read-<id>.md
```

```
Workflow({name: "plan-v2-prove", args: {slug: "<slug>", milestones: [{id: "<id>", tasks: [14, 15]}]}})
```

A result carrying `error` stops the step: report it, and never fall back to the Agent tool. Otherwise, in the same turn, record the launch under the milestone's step name and end the turn:

```bash
hash=$(sha256sum <scratch>/elaborate-prove-args.json | cut -d' ' -f1)
tomlctl plan run start --slug <slug> --step cold-read:<id> --workflow plan-v2-prove --args-hash "$hash" --run-id <runId> --session "$CLAUDE_CODE_SESSION_ID"
```

**On the notification**, close the run by write-and-prove's outcome table (`ok`, `errors` or `stopped`), then check the tree with the snapshot's exact arguments:

```bash
tomlctl plan run finish --slug <slug> --step cold-read:<id> --outcome <outcome>
bash <skill-dir>/scripts/fanout-check.sh check <scratch>/fanout-cold-read-<id> <slug> --exclude <path> --exclude docs/flows/<slug>/notes/run docs/flows/<slug>/notes/cold-read-<id>.md
```

Exit 1 or 2 stops the elaboration, as open-and-challenge.md section 3 says: show the user the output, change nothing yourself, and leave the milestone deferred. Read the index entry by write-and-prove's status table; an entry that fails its one relaunch is recorded as the "without a cold read" decision of the same section, and the milestone may then be flipped only on the user's say-so, since no approval gate will show that decision.

**Apply the points** by write-and-prove's "Apply the results": read each against the code it cites, settle what a worker would have guessed, replan each false premise, adopt a reported command as a `check` only when it asserts the stated `behaviour`, and take nothing that touches intent from a note. Every fix is a `tasks update --ndjson` with a `reason`. Then run 4a again. Rerun the cold read only under write-and-prove's "Rerun a milestone's cold read" rule, with the note deleted, a fresh exclude list and a fresh snapshot.

### 4d. Probes

Probe this milestone's accept entries, then its gate probes, by [write-and-prove.md, section 4d](write-and-prove.md#4d-probes). `plan/probe-absent` never reports a row added after approval, so nothing else will notice an elaborated row that was never probed: this step is the only guard.

```bash
tomlctl plan probes --slug <slug> --emit --milestone <id> > <scratch>/elaborate-probes.sh
timeout 540 bash <scratch>/elaborate-probes.sh > <scratch>/elaborate-probes.out
tomlctl plan probes --slug <slug> --record <scratch>/elaborate-probes.out --milestone <id>
tomlctl plan probes --slug <slug> --emit --gates --milestone <id> > <scratch>/elaborate-gates.sh
timeout 300 bash <scratch>/elaborate-gates.sh > <scratch>/elaborate-gates.out
tomlctl plan probes --slug <slug> --record <scratch>/elaborate-gates.out --gates --milestone <id>
```

Read each script before running it. Act on every verdict by write-and-prove's verdict table until every entry is `healthy` and no gate reads `fails` or `broken`.

## 5. Flip

Only now, with the cold read closed and applied and every probe healthy, mark the milestone elaborated. It is the last write to `tasks.toml` in this procedure:

```bash
tomlctl plan milestone update --slug <slug> --id <id> --elaborated --reason '<what-landed>'
tomlctl plan check --slug <slug> --milestone <id>
```

`--reason` names what landed: the decisions, the rows added and revised, and the cold read's outcome. The check runs once more because `plan/accept-no-check` now applies to these rows. An error here is fixed with `tasks update --ndjson`, and the entries that fix touches are probed again by 4d before the commit; the milestone is already runnable, so do this before ending the turn.

**Before the flip**, since no approval follows an elaboration, list the decisions that are agent-assumed, one-way and never surfaced:

```bash
tomlctl items list .claude/flows/<slug>/plan.toml --array decision --where provenance=agent-assumed --where door=one-way --where surfaced=false --select id --lines
```

Surface each through `AskUserQuestion`, recorded by `plan decide`, before the flip command above runs.

## 6. Finish and commit

```bash
tomlctl plan run finish --slug <slug> --step elaborate:<id> --outcome ok
```

Commit once, with the message from the `commit-conventions` skill, staging each path by name, never the whole flow directory: `.claude/flows/<slug>/plan.toml`, `.claude/flows/<slug>/tasks.toml`, both `.sha256` sidecars, every file this elaboration wrote under `docs/flows/<slug>/design/`, the `<name>.md` files and the PNG or text-grid pictures beside them included, and `docs/flows/<slug>/notes/cold-read-<id>.md`.

```bash
git add -- <each-path>
git commit -m "<message>" -- <each-path>
```

`tasks.toml` is one file, so the commit also carries whatever the other session's running rows read at that moment; its own flow-store commit records them later. A store write from the other session landing between the two `git add` reads leaves a store and sidecar that differ, and the pre-commit hook refuses the commit: stage both again and commit. Never `--no-verify`.

**A concurrent flow-store commit.** While this procedure runs, a `/implement-v2` in another session may make its flow-store commit, which stages all of `.claude/flows/<slug>/` and so sweeps in this session's half-revised rows. That is safe: the milestone stays deferred until section 5, so no committed row of it is runnable, and this procedure's own commit records the finished state.

**The close.** Report the milestone elaborated, the rows added and revised, the cold read's outcome and the probe verdicts, and list every `agent-assumed` decision this elaboration recorded: no approval gate will show them, so the close is where the user sees them. No re-approval is needed unless the brief changed. The last line names `/implement-v2 <slug>`, which picks the milestone once the schedule reads it `runnable`.

## 7. Re-entry and open runs

`/plan-v2 <slug> --elaborate <id>` on a milestone the schedule reads `elaborating` finds an open `elaborate:<id>` row. Read it, and any open `cold-read:<id>` row, with `session` and `run_id`:

```bash
tomlctl plan view --slug <slug> --for digest --select stage_inputs.runs
```

- **This session's own run** resumes where the store stands: decisions recorded, rows written, a `cold-read:<id>` row open or closed, probe verdicts recorded, the milestone's `elaboration`. Each section above re-runs safely from its start; skip what the store shows done.
- **An open `cold-read:<id>` run** follows write-and-prove's "Relaunch" rule, with `<scratch>/elaborate-prove-args.json` as the args file. Because this launch's snapshot took the excludes, its fan-out check lets HEAD move through another session's milestone and flow-store commits. Otherwise abandon it with `tomlctl plan run abandon --slug <slug> --step cold-read:<id> --reason '<why>'`, delete the note unless it opens `Verdict:` and no row changed since, and launch afresh.
- **Another session's run** is never abandoned silently. Ask the user through `AskUserQuestion` whether that session is still working, with leaving it as the default. Only on the user's answer that it is dead, abandon both of its rows and start afresh from section 1:

```bash
tomlctl plan run abandon --slug <slug> --step elaborate:<id> --reason '<why>'
```

A `/plan-v2 <slug>` without `--elaborate` leaves `elaborate:<id>` and `cold-read:<id>` rows alone, as `/implement-v2` does: they belong to the session elaborating that milestone.
