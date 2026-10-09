---
description: Implement a plan or task using parallel sub-agents with research, progress tracking, and verification
argument-hint: [--flow <slug>] [plan path or task description]
---

# Implementation

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

Implements a plan, feature, or task by delegating to parallel sub-agents — work decomposition, research for novel steps, efficient parallelisation, and verification. Accepts plan files, plan directories, specific items (`items 3,4,5 from …`), inline task descriptions, or no arguments (auto-resolves the active flow via the Step-0 envelope).

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning, tool usage, and deviation detection.

## Step 0: Pre-flight (flow resolution + doctor)

Invoke the `flow-contract-flow-context` skill to load the flow-bootstrap envelope contract (input/output shapes, `envelope.ok` gating, `envelope.resolved.*` and `envelope.doctor.*` binding rules, no-flow fallback, doctor-fail handling, staleness reconciliation, and the mandatory bootstrap-summary console line).

Build the input envelope:

```bash
tomlctl flow envelope build \
  --command implement \
  --branch "$(git branch --show-current)" \
  --worktree "$(git rev-parse --show-toplevel)" \
  --cwd "$(pwd)" \
  --require-artifact execution_record \
  --staleness-threshold 7d
```

The block above is complete and copy-pasteable as-is — do NOT look up `--help`. The `--require-artifact execution_record` flag is what pins `require_artifacts = ["execution_record"]` in the emitted envelope (`/implement` reads the record before writing it); `--staleness-threshold 7d` is the default, passed explicitly for clarity. On detached HEAD, omit `--branch` so the envelope records `branch:null`. Add `--flow-override <slug>` when the user supplied `--flow`, and `--path-arg <p>` once per `$ARGUMENTS` path token. Dispatch `flow-bootstrap` via the Task tool with `subagent_type: "flow-bootstrap"` and the printed JSON as the prompt. Gate on `envelope.ok`; bind `slug`, `context_path`, `artifacts.*` (esp. `execution_record`), and `doctor.ok` for downstream phases. Emit the bootstrap-summary line before any other action. On no-flow, prompt the user per `envelope.warnings` / `tie_candidates`.

## Step 0.5: Ensure the task store

Invoke the `flow-contract-task-store` skill to load the store contract (the schema and every row field, the `ref` rule, the status vocabulary, the stored-vs-derived edge split, the semantics of each `tasks` verb, the `check` finding classes and exit policy, the render contract and its containment guarantee, the ref-set diff gate, the orchestrator-only status-write rule, fetch-by-id dispatch, the degradation halt, and `tomlctl integrity refresh` as the recovery after a failed write).

`.claude/flows/<slug>/tasks.toml` is the flow's task DAG, and from here on it is what this command schedules from: the frontier, the file-claim check, the checkpoint closures and the idempotency skip-list are all verbs against it rather than prose re-derived from the plan. Bind `<store>` as `envelope.resolved.artifacts.tasks` (fallback `.claude/flows/<slug>/tasks.toml`); every `tasks` call below targets it with `--slug <slug>`.

**`/tdd` sub-flows are exempt, wholesale.** A slug matching `<parent>-tdd-<NNN>` never creates, imports or reads a store — it keeps the record-derived skip-list named in Phase 1 and every pre-store rule this command carries. The parent flow owns the plan and therefore the store; a sub-flow importing the parent's plan into its own directory would mint a second store with the same refs and no owner. Skip this whole section and every `tasks` invocation below for such a slug.

Otherwise import before anything reads a row status — on a first run because the store is absent on disk or holds no rows, and on **every resume** because a populated store is not self-evidently in step with the record:

```bash
tomlctl tasks import-plan --slug <slug> --reconcile-record
```

`--reconcile-record` is what stops a fresh import re-dispatching work the execution record already holds as `done`: it promotes a row whose `task_ref` matches a `status = "done"` completion, adopts the record's spelling of a `ref` on a unique normalised match, and reports every record `task_ref` it could not place in `unmatched_refs`. Surface those; never guess a match. **Re-running it is the resume path's repair, and is idempotent**: the upsert is keyed on `ref` and only ever promotes a row to `done`, never demoting one, which is what recovers a row still `pending` under a completion `<record>` already holds — the shape a `/plan-update status`, `reconcile` or `migrate` run leaves behind, since each derives from `<record>` and none writes a row status. Phase 1's skip-list is store-derived, so without this the resume re-dispatches that task. The envelope's `--require-artifact` list stays as Step 0 has it — naming `tasks` there would fail a flow minted before the store existed, which is exactly the flow this import is for.

**Degradation is a halt, never a fallback.** Halt the run with a named diagnostic when `tasks check` reports an error-class finding, when the store is still absent after an import that claimed to succeed, or when `tomlctl tasks` is unrecognised (a stale binary on `PATH`). The diagnostic names both recoveries: `cargo install --path tomlctl` for the stale binary, and a `git revert` of the four carrier adoptions (`/plan-new`, `/implement`, `/review-plan`, `/plan-update`) to fall back to the prose frontier. A `backlog/unknown-id` error — a task's `Backlog` bullet naming an id `.claude/backlog.toml` holds in neither its live nor its compacted array — has its own recovery: sync `.claude/backlog.toml` (a worktree cut before the item was minted lacks it), or drop the id from the task's `Backlog` bullet and re-import. Never proceed on partial store output — a frontier computed from a store that failed its own checks dispatches the wrong tasks in the wrong order, and it does so silently.

## Phase 1: Analyse and Decompose (main conversation — thinking enabled)

**Reason thoroughly.** Front-load analysis here. Read the resolved flow's `context.toml`, extract `plan_path`, and read the plan.

**Plan-path validation (mandatory, before any plan Read).** The plan's narrative sections — Summary, Context, Scope, User Decisions, Approach and Risks, whichever of the six the plan carries — are embedded verbatim into every Phase-2 agent prompt's shared preamble. The `## Exploration Notes` / `## Research Notes` appendix (or `RESEARCH-NOTES.md`) is never embedded: a task that relies on design text names its `###` Approach sub-heading, which the preamble already carries. Resolve the candidate path and verify it falls under the git top-level. **Reject** if: (1) it contains `..` after normalisation; (2) it is absolute and not under the git top-level prefix (`/tmp`, `/etc`, `~/`, etc.); (3) it resolves outside the repo via symlink. Halt naming the offending path; dispatch no agent. This binds to the initial read, outline/detail reads, and any Phase-4.5 re-read.

Handle plan-directory (start at outline/master, then only relevant detail docs), single-file, inline-task, and item-subset (`items 3,4,5`) inputs. Update the resolved `context.toml` in **one** `set`: read pre-update `status` as `<old_status>`, then set `status = "in-progress"`, `[tasks].total` to the store's row count and `[tasks].in_progress` to the count of tasks this run will dispatch, preserving `created` and key order. Every `--set PATH=VALUE` pair lands in the same write as the positional pair. Pass no `updated` pair: a `set` on a file named `context.toml` refreshes its root `updated` to today whenever it changes anything else. `set` infers each type — the counts land as integers — so pass no `--type` and no fallback:

```bash
tomlctl get <context_path> status --raw
tomlctl set <context_path> status in-progress --set "tasks.total=$(tomlctl tasks list --slug <slug> --count --raw)" --set tasks.in_progress=<n> -q
```

**`[tasks].in_progress` is derived from the frontier scheduler's in-flight set.** It is written only by `/implement`; `/plan-new` and `/plan-update` never touch it, and Phase 4.5 resets it.

Invoke the `flow-contract-execution-record-schema` skill to load the canonical execution-record contract (schema, type vocabulary + per-type required fields, the `flow record` write contract, `<record>` path-resolution rule, `[[items]]` subcommand restrictions, append-only/supersession, deterministic PROGRESS-LOG.md regeneration via `tomlctl flow render-progress-log`, `[tasks].completed` derivation, read-path `--verify-integrity` integrity contract, field-length caps, and read rules). Resolve `<record>` as `[artifacts].execution_record` (fallback `.claude/flows/<slug>/execution-record.toml`) and use it, fully qualified, for every read — never the bare filename. **Every append to `<record>` is one `tomlctl flow record --slug <slug>`**, which resolves the same path itself, mints the `E<n>` id, defaults `date` to today, validates the type, its required fields and enums, caps over-long text and restamps `last_updated` — so no write follows it, and no id, date or JSON escaping is ever done by hand. No explicit pre-create is needed: `flow init` / `/plan-new` normally pre-seed the record, and if it is absent on disk the first `flow record` auto-creates it with the `schema_version = 1` + `last_updated = <today>` skeleton. Pass each field as `--set KEY=VALUE` (a string; a digit string is coerced where the schema wants an integer) or `--set-json KEY=JSON` (the arrays — `files`, `commits`, `failed_ids`); multi-line prose goes into a file written with the Write tool and in through `--set-file KEY=<path>`, as does a value starting with `/`, which Git Bash would otherwise rewrite into a Windows path. If `<old_status> != "in-progress"`, append a `type=status-transition` entry (skip the no-op case):

```bash
tomlctl flow record --slug <slug> --type status-transition --set agent=implement --set from_status=<old_status> --set to_status=in-progress --set summary="status <old_status> -> in-progress"
```

Build the idempotency skip-list from the store — `tomlctl tasks list --slug <slug> --where status=done --pluck ref` — and skip every task whose `ref` it names. **A record `task_ref` is the store row's `ref`**, and `flow record --task <id>` is what writes it, copied from the row — never type a `task_ref` by hand, since the `ref` rule drops characters a hand-derived slug keeps (a `.` in a heading, for one). The execution-record contract pins the two to one value, which is what lets Step 0.5's `--reconcile-record` import carry a prior run's completions into row statuses, and what makes this one query equivalent to the record scan it replaces. A `/tdd` sub-flow, having no store, keeps that scan: `tomlctl items list <record> --where type=task-completion --where status=done --pluck task_ref --verify-integrity`. With no store row to copy from, its `flow record` calls pass `--set task_ref=<ref>` in place of `--task`. Extract `## Verification Commands` for Phase 3, and the plan's `## After Merge` list for Phase 4.

Research novel/complex steps now (Context7 + WebSearch), resolve ambiguities, and classify each task Straightforward vs Complex. **The DAG, the execution policy and the checkpoint groups are read from the store, not re-derived from the plan prose** — the plan's `## Tasks`, `## Execution Policy` and `## Dependency Graph` sections are rendered from it. Gate on `check` before binding anything, then bind `checkpoint_policy ∈ {single, milestones, per-batch}`, `max_parallel` (default 6, ceiling 8), and `commit_granularity ∈ {per-task, per-checkpoint, single-commit}` (default per-task) from `[policy]`, and the checkpoint groups from `[[checkpoints]]`:

```bash
tomlctl tasks check --slug <slug>
tomlctl get <store> policy
tomlctl tasks batches --slug <slug>
```

Any error-class finding halts per Step 0.5's degradation rule; warnings are surfaced and carried into the Phase-4 report. `dag/unreachable-claim` is the one to read closely — it names the file-claim collisions the frontier will hold on at dispatch. **A `/tdd` sub-flow, or a legacy flow whose plan carries no `## Execution Policy`, binds legacy mode**: per-batch cadence, per-checkpoint granularity, `max_parallel` 6. Only `max_parallel` coincides with the store's own fallback — an absent or partial `[policy]` table falls back to milestones / 6 / per-task — so legacy mode is bound *in place of* the three stored values, never read from them. **The discriminator is `[policy].origin`** in the table the `tomlctl get` above already returned, because the import always materialises a `[policy]` table and a pre-policy plan is otherwise indistinguishable afterwards: a plan carrying no `## Execution Policy` imports as `origin = "default"` — every field a house default, and a `plan/policy-absent` warning raised at the import that substituted them — while `origin = "plan"` means the plan authored the section and `[policy]` is read as-is. **Never test `[policy].note` for mode**: it is authored prose, and an absent policy section leaves it empty. A `/tdd` sub-flow has no store at all and binds legacy mode without the check.

**Unclaimed promotions.** List the backlog items promoted to this flow, read-only:

```bash
tomlctl backlog reconcile --flow <slug>
```

Surface each `buckets.unlinked` id as `N backlog item(s) promoted to <slug> are claimed by no task: <ids>` and carry it into the Phase-4 report. It is a warning, not a halt — no task closes those items, so Phase 4's reconcile will not resolve them; the remedy is a `Backlog` bullet on the task that delivers each, or `/backlog` re-triage.

## Phase 2: Execute (parallel sub-agents)

**Frontier scheduling.** The frontier is a verb, not a re-derivation. Pass the ids of every dispatched, non-terminal task as `--in-flight`:

```bash
tomlctl tasks ready --slug <slug> --in-flight 3,4
```

It answers `{ready[], held[{id, blocked_on_file, holder}], next[], blocked[{id, blocker, blocker_status}]}`. **The file-claim check is inside the verb:** `ready` already excludes every row sharing a file with an in-flight one, so it is directly dispatchable, and `held` names each excluded row with the file and the holder that blocks it. Two ready tasks sharing a file run one after the other, never together — pass the ids being dispatched in this same response into the next `--in-flight` before asking again. A `held` entry reveals a missing dependency edge in the plan: do not silently mint the edge — record it and surface it in the Phase 4 report under `### Plan Deviations` (assumption: tasks independent; found: file overlap, serialised at dispatch). Dispatch every id in `ready`, up to `max_parallel` in flight — **all currently-ready tasks MUST be emitted in the same assistant response**: N ids in `ready` ⇒ N `Agent` blocks before the turn ends, no fewer; deferring one to a later turn serialises it for no reason. Do NOT reduce the agent count. As each agent completes, process its return (steps 3a/3b/5b and the escalation handler below), move its row to a terminal status, re-run `tasks ready` with the shrunken `--in-flight` set, and dispatch newly-ready tasks **immediately in the same turn** — never hold a ready task waiting for unrelated in-flight work. The only legitimate pause is a checkpoint drain (step 5). Under per-batch legacy mode this degrades to the classic dependency-level batches — `tomlctl tasks batches --slug <slug>` gives the levels, each fully dispatched in one response, gate + commit between levels. Place shared context (the plan's narrative sections — Summary, Context, Scope, User Decisions, Approach, Risks — + common constraints, never its `## Tasks` prose nor its notes appendix — the store owns that) as a byte-identical literal preamble atop every agent prompt for the whole flow (prompt-cache reuse), with per-agent divergence below a divider.

**An exhausted frontier is not completion.** `blocked` names the rows no later wave can reach — each with the **nearest ancestor** that is neither `done`, nor `pending`, nor in the `--in-flight` set just passed. The walk climbs through intervening `pending` rows, so `blocker` is always the stall itself and never a row between: the id it names is the one to act on, and one stall names itself once per pending row behind it. All four keys empty means the run is finished; `ready`, `held` and `next` empty while `blocked` is not is a **stall**, and treating it as completion ends the run silently short of the plan. Read each entry's `blocker_status` before deciding: `failed` is the expected shape after a step-6 rollback, and those dependents stay blocked by design. `in-progress` on a `blocker` the `--in-flight` set does not name is a **crashed dispatch** — no agent holds that blocker, so nothing will ever move it and the wait is unbounded. Route the blocker to the resume path rather than waiting on it: settle it `done` if `<record>` holds its `task-completion`, else re-dispatch it as an unstarted task. A declared in-flight ancestor never stalls, so a task genuinely being worked keeps its dependents in `next`.

**Dev server (UI tasks).** Decide once, before the first dispatch, so the preamble stays byte-identical. When a task this run will dispatch has an `Acceptance` naming something visible and the project's `.mcp.json` declares the `playwright` server, probe the dev URL the project documents (its CLAUDE.md, or the dev script in its manifest); if nothing answers and a dev command is documented, start it in the background. Put `DEV SERVER: <url>` in the preamble, or `DEV SERVER: none`. The implementers attach only to that URL and never start a server of their own; stop a server this run started before Phase 3.

**Agent dispatch rules — fetch by id, not paste.** Below the byte-identical preamble, a prompt carries the task's **id** and the command that fetches its own body:

```bash
tomlctl tasks show <id> --slug <slug> --with body,files,deps
```

That is the store's whole point: `## Tasks` is the largest single thing an orchestrator loads, and only the agent executing a task needs that task's prose. `--with deps` gives it the direct dependency summaries it needs to know what already exists; `--with dependents` is a planning read, not a dispatch read, and grows with the plan. **A sub-agent gets the read verbs only** — `show`, `list`, `edges`, `ready`, `batches`, `closure`, `check`. Every store write is the orchestrator's: two writers means a row's status and the record's `task-completion` entries can disagree, and the store has no supersession mechanism to reconcile them.

Beyond the id and that command, a prompt carries only what the agent can neither fetch nor already holds: the Phase-1 research for a complex task and the exact API signatures it settled, so the agent checks them rather than rediscovering them; and the files its in-flight siblings hold, so it can tell a sibling's breakage from its own. The implementers' system prompts carry the per-item workflow, the escalate reasons, the report lines (`deviation:`, `note: check`, `## Files touched` with its `(new)` marker), the trigger-based external-docs rule and the build limit — do not restate them. **A `/tdd` sub-flow has no store**: in place of the id and the fetch command, paste its single task's Action, Acceptance and Files, and state `no task store — do not run tomlctl tasks`.

**Falsifier check (every prompt).** Each prompt MUST also direct: *for every assertion you add or change, confirm it actually fails against the pre-change behaviour* — restore the old constant, comment out the new line, or feed the input the assertion is meant to reject — then observe the failure, report the observed failure count, and restore. An assertion that passes both before and after your change is guarding nothing, and both sides of a comparison being `undefined` is the common shape. Do this **in-editor only: never `git stash` / `reset` / `checkout --` / `restore`** — those are orchestrator-only (see the stash-escalation handler), and siblings are editing the same tree with up to `max_parallel` agents in flight. **Do NOT emit `escalate <id>: stash-required` for this check**: retype the prior value from your own edit, which you already know — you do not need the on-disk pre-edit state, and that escalation pauses frontier dispatch until every in-flight agent terminates. Agents that volunteered this check have historically caught real vacuous assertions that every declared gate passed.

**Lite-eligibility gate (per-task dispatch).** Pick `implement-deep` (default) or `implement-lite` **per task**. Effort tag is primary: `S` is lite-eligible subject to the criteria; `M`/`L` ALWAYS go deep (do not evaluate the rest); no/unknown tag ⇒ treat as `M`. The four criteria (S only): (1) ≤ 2 files; (2) action fully specified, no design decisions left; (3) no cross-file refactor; (4) not security-sensitive (auth/crypto/input-validation/sandbox/token or session handling). Coupling-isolation: when tasks are dispatched as one coupled cluster (one coherent change split across prompts), a single failing task sends the whole cluster deep — do not peel tasks out of a cluster; independent file-disjoint tasks that merely share a response are NOT a cluster and gate individually. Record the choice as a one-line `DISPATCH:` header atop each agent prompt.

For each dispatch: move each dispatched row to `in-progress` and record the tier's agent on it (re-check the all-ready-tasks-emitted invariant before ending the turn). Group the rows dispatched in one response by tier and issue one call per `--agent` value, listing every id that tier took; one call writes all its rows, and an unknown id aborts it with nothing written. The store write is what makes the next `tasks ready` correct — a row left `pending` while its agent runs is offered again.

```bash
tomlctl tasks update <id>,<id> --slug <slug> --status in-progress --agent implement-deep
tomlctl tasks update <id>,<id> --slug <slug> --status in-progress --agent implement-lite
```

**Record the dispatch-time file state.** Before the agents are dispatched, ask the store which of each row's `files` are missing under the repo root, in one call for every id going out in this response, and emit one console line per row in this conversation: `dispatch <id>: absent at dispatch — <paths>` (or `— none` for an empty `absent`). `tasks show` has no `--ndjson`; `--lines` prints one compact row per line:

```bash
tomlctl tasks show <id>,<id> --slug <slug> --with absent --select id,absent --lines
```

That line is the record. It is orchestrator-held state that no store field carries, and step 6's new-file test reads it back from this conversation to decide which untracked paths the rollback stashes away and which it leaves on disk. When the `cross-cut` handler widens the row's `files`, re-run that call for the row before the re-dispatch and emit the line again, carrying forward the absent paths of the earlier line and adding only those of the newly added paths its `absent` names. Never take a fresh answer for a path already recorded: a retry or reroute would otherwise count a file the failed attempt created as present at dispatch, and step 6 would leave it on disk. A run resumed in a fresh conversation has no such line for a row still in flight. Step 6 then treats every path on that row as present at dispatch.

Per completed agent:

- **3a. Vet `implement-lite` output before promoting completion.** A vet reads the touched lines and confirms they match `Action`+`Detail` with the surrounding style preserved. **Flagged — always vetted:** a lite return carrying `applied <id> [vet-recommended]` (start from what its `uncertain:` reason names), `note: check not run` on a non-trivial edit, an added or changed assertion with no observed falsifier failure count or a count of 0, a `deviation:` line, or `skipped <id>: already-applied` (read its cited `file:line` and confirm the `Action`'s post-state is there — no citation fails the vet). **Sampled:** of the unflagged lite returns, vet 1 in 2 in run order (the 1st, 3rd, 5th…); drop to 1 in 4 once this repo's execution records hold ≥ 30 lite-tier completions with a `sampled-*` or `flagged-*` `vet` written since `implement-lite`'s `model:` or `effort:` last changed, under 5% of them `*-fail` — a retune restarts the count. A failed vet ⇒ re-dispatch the task to `implement-deep` (counts as a failed retry; 5b records it with `escalation_reason = "vet-failed"`, superseding the task's earlier entry if it had one), vet every unvetted lite return of the current checkpoint window before its gate, and vet every lite return for the rest of the run. Skip vetting for deep tasks. Do NOT append a `task-completion` for a task whose vet failed until its re-dispatch settles.
- **3b. Plan deviations.** Each `deviation: <id> — <file> — spec said <planned>, code required <done>: <why>` line in a return becomes one `type=deviation` entry in `<record>`, written with `--task <id>` so its `task_ref` is the row's: `original_intent` = `<planned>`, `rationale` = `<why>`, `summary` = `<done>` in `<file>`, `commits` per 5b. **Pre-append dedupe guard (mid-batch-crash safety):** query for an existing match on the `(task_ref, original_intent, rationale)` triple, with the row's `ref` as `<ref>`; append only when the count is 0:

  ```bash
  tomlctl items list <record> --where type=deviation --where task_ref=<ref> --where original_intent="<planned>" --where rationale="<why>" --count
  tomlctl flow record --slug <slug> --type deviation --task <id> --set agent=implement --set original_intent="<planned>" --set rationale="<why>" --set summary="<done> in <file>"
  ```

  Significant deviations pause and surface to the user. Do NOT advise a second `/plan-update deviation` for an already-recorded deviation.
- **3c. Minting a task the plan lacks.** When execution reveals necessary work no task owns (a repair for a contradiction between two acceptances, a call site for a seam, a mitigation the Risks section describes but assigns to nobody), mint it into the store rather than smuggling it into an adjacent task's diff. `tasks add` mints the id and the `ref`, and refuses a dangling dependency target or a cycle before writing anything:

```bash
tomlctl tasks add --slug <slug> --title "<title>" --effort S --files <f1>,<f2> --needs <ids> --checkpoint <group> --action "<what to do>" --acceptance "<how it is verified>"
```

The returned `batch` is the Kahn round the row lands in; the next `tasks ready` schedules it through the normal frontier. Give it the checkpoint group whose closure it belongs in — usually the one holding the task that forced the mint — and pass a group `[[checkpoints]]` actually declares: `add` refuses an undeclared id outright, naming the ones the store holds, so a mistyped group costs a re-issued command rather than a row nothing commits. Omitting `--checkpoint` is the legal "no group" spelling, and such a row is committed only by the Phase-3 train, which `tasks check` reports as `checkpoint/orphan-task`. Append a `type=deviation` entry recording the mint (`original_intent = "plan carries no task for <X>"`, `rationale` = what forced it). Then re-read `[tasks].total` from `tomlctl tasks list --slug <slug> --count` and write it to `context.toml`. **The plan document is behind the flow until Phase 4 renders it**: the render writes the minted task into `## Tasks` from the store, so name the divergence in Phase 4's Next Steps (`plan document described N tasks; N+K executed`) and confirm the render carried it — recommend `/plan-update reformat` only where the render was skipped or refused. Never silently widen a sibling task's scope to absorb the work — that destroys the file-claim invariant the scheduler depends on.
- **4.** Move each returned row to its terminal store status — `--status done` on success, `--status failed` once the retry budget is spent. Rows settling to the same status together share one call with an id list.

  ```bash
  tomlctl tasks update <id>,<id> --slug <slug> --status done
  ```

  Execution continues (dependents stay blocked). Before `git commit`, apply the `commit-conventions` skill if installed and emit the sentinel `IMPLEMENT-AUTOCOMMIT: phase-2-step-5`.
- **5. Checkpoint: drain → gate → commit train.** Checkpoints fire per the bound policy: `per-batch` — after each dependency level that has dependents (legacy behaviour; if no dependent batch follows, skip gate and commit — the Phase-3 pass covers it); `milestones` — when every member of a declared checkpoint group is terminal, the membership being the verb's answer rather than a prose closure; `single` — never mid-run (Phase 3 is the only gate; step 5b entries carry empty `commits[]` until the final train). At a checkpoint:

  ```bash
  tomlctl tasks closure --slug <slug> --checkpoint A
  ```

  `members[]` is the group, `maximal[]` its `Checkpoint after` antichain, and `valid_cut` says whether the group is committable *here* — it covers the union with every earlier group, so `false` means the prefix is not downward-closed and the window would commit a task whose dependencies are still open. Surface a `false` as a plan defect and do not commit against it.

  - **(i) Drain.** Stop dispatching new tasks and let in-flight agents finish. The gate needs a quiescent tree — never build/test while agents are editing.
  - **(ii) Gate.** Dispatch the `verification` agent (`subagent_type: "verification"`) with the checkpoint command list — the plan's `checkpoint:` commands when it declares them, else `build:` then `test:` narrowed to the crates or packages the window touched (`--manifest-path <crate>/Cargo.toml`, `-p <pkg>`), else CLAUDE.md's documented commands — carrying each command's `timeout:` / `rerun:` options and any `transient:` patterns from the plan. Checkpoints run the fast tier: `e2e:` waits for Phase 3 unless `checkpoint:` names it, and `success:` commands always wait for Phase 3. The agent stops at the first `fail` or `timeout`. Judge each block; never rerun it yourself:
    - **`pass`** needs `exit: 0`, and for a test command a `summary:` showing at least one test executed and none failed. **A `summary: none` or a zero count** (`No test files found`, `0 passed`, `no tests to run`) **is a `fail`**: a path filter matching nothing exits 0 in most runners, so a mis-rooted or mistyped command reads green on a suite that never ran. On an ambiguous block, `Read` at most ~40 lines of its `log:` or re-dispatch that one command.
    - **`flaky`** is a pass — the failed tests passed on the agent's narrow rerun or the runner's own retry. It consumes no fix-and-reverify cycle, reruns nothing, and never triggers step 6; carry its `TANGENTIAL: flaky-test` lines to Phase 4's backlog harvest.
    - **`timeout`** says nothing about the code: re-dispatch only that command plus the `not_run:` tail, with a larger `timeout:` or split per crate, binary or shard. No fix cycle is consumed.
    - **`fail`** — a test block with 1–10 `failed_ids:` and no `rerun:` line first gets one re-dispatch narrowed to those tests (green ⇒ `flaky`). Otherwise diagnose in the main conversation from `tail:`, `failed_ids:` and the log, fix directly or via a targeted agent (counts against the retry budget — max 2 fix-and-reverify cycles), then re-dispatch the whole list; **do NOT commit a red window** — if it cannot go green within budget, go to step 6.

    **Never run a build or test command in the main conversation** — not to corroborate a pass, not to diagnose, not for fear of a slow suite. Its output floods the orchestrator's context, and time budgets are the agent's to enforce: it has no wall-clock limit of its own. This judgement is the orchestrator's — leave the agent's run-and-report contract alone. Once every block is `pass` or `flaky`, append one `type=verification` entry per executed command (`command`, `outcome`, plus `duration_s` and `failed_ids` when the block carries them; `--task` = the checkpoint's lead task). Several commands go in one call — stage one JSON object per command (its `command`, `outcome` and `summary`, plus the optional two) in a file with the Write tool and pass it as `--ndjson`; the field flags and `--task` apply to every row, and the batch is all or nothing:

    ```bash
    tomlctl flow record --slug <slug> --type verification --task <lead-id> --set agent=implement --ndjson <staged-rows-path>
    ```
  - **(iii) Commit train (selective staging).** The groups and their order are a verb, not a hand partition. `tasks train` takes every `done` row with an empty `commit`, groups them per `commit_granularity` (one per task by default; one per checkpoint group under `per-checkpoint`; one for the window under `single-commit`), **merges any groups sharing a file** (their edits are no longer separable in the tree) along with any whose dependencies run both ways, and prints the groups in commit order, each `{ids, refs, files, checkpoints, shared_with_pending}`:

    ```bash
    tomlctl tasks train --slug <slug> --lines
    ```

    Run it **without `--checkpoint`** at a milestones checkpoint. The frontier keeps dispatching while a group drains, so tasks of a later group are often already `done` when an earlier checkpoint fires, and their files can overlap the closing group's: narrowed to one checkpoint, the train would stage a file whose tree also holds an uncommitted later edit. Unnarrowed, the merge sees every done-and-uncommitted row and folds them into one group instead. (`--checkpoint X` and `--ids N,…` narrow the window when that cannot happen; `--granularity` overrides the stored policy.)

    Per group, in the order printed: apply the `commit-conventions` skill, emit the sentinel `IMPLEMENT-AUTOCOMMIT: phase-2-step-5`, stage EXACTLY the group's `files` and commit them — `git add -- <path>...` then `git commit -m <msg> -- <path>...`, NEVER `git add -A` / `git add .` / `commit -a`. The `--` pathspec on the commit keeps any unrelated entry already in the index out of it. **Read `shared_with_pending` before staging a group**: it names each group file that a row not yet `done` also claims. Look the claimants up (`tomlctl tasks list --slug <slug> --where files=<path> --where-not status=done --select id,status --lines`). An unstarted `pending` claimant has made no edit yet, so the file is committed as it stands and the claimant edits it later. Any other claimant — `in-progress` after a crashed dispatch, `failed` with work the rollback left in place — may have an edit in that file the commit would sweep in: halt the group and surface it to the user. Dirty paths claimed by no row are never committed; unexplained leftovers halt the train for user review. Only the train's tip is gate-verified — intermediate commits are logical splits of a verified tree, attributable via the execution record's SHA→task_refs map. **If any `git commit` fails (e.g. a pre-commit hook rejects it): halt the train, surface the failure to the user, and do not record that SHA against any task.** After each group commits, record its SHA on the group's rows in one call — the update envelope echoes each row's `ref`, which is the group's half of the SHA→task_refs map:

    ```bash
    tomlctl tasks update <id>,<id> --slug <slug> --commit <sha>
    ```

    After the train, append one `type=checkpoint` entry with the train's SHAs and a `summary` mapping each SHA to its group's `refs`, then resume frontier dispatch:

    ```bash
    tomlctl flow record --slug <slug> --type checkpoint --task <lead-id> --set agent=implement --set kind=commit-train --set-json commits='["<sha>","<sha>"]' --set summary="<sha>: <ref>, <ref>; <sha>: <ref>"
    ```
- **5b.** Per terminal-state task, append a `type=task-completion` entry **at completion time** (crash-safe — Step 0.5's `--reconcile-record` reads these entries on the next resume to repair a row a crash left non-terminal, so an entry batched to the checkpoint is one a crash loses). **Write it with `--task <id>`**, which copies `task_ref` from the store row's `ref` and refuses a payload `task_ref` that disagrees — never pass `task_ref` yourself; the execution-record contract pins the two to one value, and that join is what Step 0.5's `--reconcile-record` import and Phase 1's skip-list both stand on:

  ```bash
  tomlctl flow record --slug <slug> --type task-completion --task <id> --set agent=implement --set status=done --set summary="<what landed>" --set-json files='["<path>","<path>"]' --set-json commits='[]' --set dispatch_tier=deep --set dispatch_agent=implement-deep --set vet=skipped --set retries=0
  ```

  Fields: `status ∈ {done,failed,skipped}` (`skipped` for a task the escalation handler surfaced instead of finishing), `files` (the paths under the agent's `## Files touched`, each stripped of its ` (lines …)` / ` (new)` annotation; `[]` for a task that touched nothing), `commits` (the real SHA when the task's commit already exists at append time — per-batch mode commits in step 5 first, so its entries carry the SHA as before; under milestones/single, `[]` — the subsequent `kind = "commit-train"` checkpoint entry is the authoritative SHA→task_refs map), `dispatch_tier ∈ {lite,deep}` (the gate's decision), `dispatch_agent ∈ {implement-lite,implement-deep}` (the agent whose return settled the task) — a lite-gated task rerouted to deep records `lite` / `implement-deep` plus `escalation_reason`, per the execution-record contract; `vet` (3a's outcome for the task's lite return — `flagged-pass|flagged-fail|sampled-pass|sampled-fail`, or `skipped` when 3a did not vet it or the task went deep) and `retries` (retry-budget spends before the task settled, `0` when its first return did). **`flow record` validates `files[]` itself** — pass the agent's paths as they came. It drops an absolute, `~`, drive-letter or `..` entry and lists it in `dropped_files`, which you echo to the console as a warning; when dropping empties a non-empty list it refuses with `kind=validation` and writes nothing, so halt that task's append and surface it (a rerun picks the task up via the skip-list). An out-of-`scope` path is written with `scope_warning = true` and listed in `scope_warnings`, a soft warning rather than a reject; over-cap text comes back truncated and listed in `truncated`. Quote each `--set` value for the shell and nothing more — the verb builds the JSON, so no escaping is done by hand.
- **6. Rollback on failure.** Re-dispatch the failed task to `implement-deep` FIRST (counts against the retry budget) — never roll back sibling tasks' successful work. Only after the budget is exhausted: for **uncommitted work** (a milestones/single window, or a task that never reached a commit), roll back ONLY the failed task's declared files — `<files>`, the row's current `files`, widenings included. File ownership is absolute, so no other task's file is ever in that set once step 2 has dropped its directory claims; never touch files owned by other tasks, and if the failed task's file set is uncertain (its `## Files touched` names a path outside `<files>`), leave that path alone, stash first and surface it. A bare `git restore -- <files>` is not enough: it exits 1 on any path git does not track (a new file, or a declared file never created) and then restores *nothing*, tracked edits included. The state it decides from is git's and the dispatch-time record's, never note or agent text alone:

  1. **Fetch the declared new files** yourself — the dispatch-time fetch was the agent's, and its output never reached this conversation:

     ```bash
     tomlctl tasks show <id> --slug <slug> --with files
     ```

     Bind `new_files` from it. A missing key (an installed `tomlctl` that predates the field) reads as `[]`.
  2. **Drop directory claims before any git call.** Remove from `<files>` every entry that ends in `/` or is a directory on disk (`test -d <path>`), and never stash, list or restore it. The scheduler compares claims as exact strings, so a task claiming `d/` can run beside a sibling claiming `d/x.rs`, and a directory pathspec would sweep the sibling's edits and new files into this rollback. Leave it alone as for an uncertain file set, and name it in the report as a directory claim the rollback did not touch.
  3. **Classify every declared path before anything moves** — the stash removes each untracked path it names, so a test run afterwards sees nothing. `<tracked>` is the output of `git --literal-pathspecs ls-files -- <files>`, never a list read from the notes. Every other path is *missing* (`test -e` fails), *ignored* (`git check-ignore -q -- <path>` exits 0), or untracked, and an untracked or ignored path *passes* only when all four hold: it is untracked now (absent from `<tracked>`); it is named absent in this row's `dispatch <id>: absent at dispatch` line (no line, as after a resume, fails this test); it is a regular file (`test -f <path>` succeeds and `test -L <path>` fails); and it is in `new_files` or the agent's `## Files touched` marks it `(new)`. Record the first test each non-passing path fails.
  4. **Stash.** The pathspec is the existing, non-ignored paths of `<tracked>` plus the untracked paths that pass step 3. A missing path makes `stash push` exit 1 with the stash already written but the tree left as it was, and a named ignored path fails it the same way, so both are left out. An untracked path that fails step 3 is left out too, so it stays on disk: an untracked declared file that already existed at dispatch — one an earlier task created in the same uncommitted window — is not this task's to remove. **An empty pathspec skips the stash**: a pathless `stash push` takes the whole tree. Otherwise run `git --literal-pathspecs stash push -u -m implement-rollback-<ref> -- <pathspec>`. `--literal-pathspecs` stops a `*`, `?` or `[` in a path matching a sibling's file, and `-u` gives each passing new file a recovery copy. On success the stash has already returned the tracked paths to HEAD and removed the passing untracked ones; immediately record its commit SHA (`git rev-parse --verify -q stash@{0}`) for the report — `stash@{0}` is positional and shifts under a second rollback in the same run, the SHA does not, and `git stash apply <sha>` takes it. `No local changes to save` means there is nothing to recover and no SHA. **Any non-zero exit halts the rollback**: surface the stash list and `git status --porcelain` for `<files>` to the user and run no step below.
  5. **Restore the tracked set.** When `<tracked>` is non-empty, run `git --literal-pathspecs restore -- <tracked>` — it restores the tracked paths the agent deleted, which the stash could not name.
  6. **Account for the untracked paths; no `git clean` runs.** Each passing path is gone, removed by the stash that holds its copy. Each failing path stays where it is. An ignored path that otherwise passes stays too, as an ignored survivor; do not delete it by any other means.

  Then mark the row `failed` per Phase 2 step 4 above, keep its dependents blocked — `tasks ready` does that on its own, since a dependent's in-degree only clears on `done` — and continue the frontier. The Phase-4 `### Failed / Skipped` entry for the task names the stash SHA (recover with `git stash apply <sha>`), each path the stash removed, each path left in place with the test it failed, each ignored survivor, and each directory claim the rollback did not touch. For **committed work** (a red checkpoint already landed, per-batch mode), `git revert` scoped to the failing items' files — successful items retain changes.

**Retry budget:** max 2 fix attempts per failure; after that mark failed, revert if it breaks the build, continue. **Cross-cutting changes:** give a 15-file rename to one agent (never split); if too large, sequence (definition+direct consumers, then indirect).

**Escalation handler (delegate-emitted `escalate`).** Route each `escalate <id>: <reason>` by its reason word before the row moves to a terminal status:

- **`ambiguous`, `security-sensitive`, or any other reason from `implement-lite`** (a failed narrow check included): the gate misjudged the task. Re-dispatch it to `implement-deep` with the escalation's evidence in the prompt; the dispatch step records the new agent on the row. A reroute is not a retry and does not spend the budget; 5b records it with `escalation_reason`.
- **`cross-cut — needs <file>`** (either tier): the plan's `Files` line omitted a file the task needs. Widen the row and hand it back to the frontier, which holds it while an in-flight task claims `<file>` and gates it afresh on the widened set:

```bash
tomlctl tasks update <id> --slug <slug> --unlock-import-fields --set files=<current>,<file> --status pending
```

  Before the re-dispatch, record the added path's dispatch-time state per the dispatch step. Append a `type=deviation` entry (`original_intent = "Files line for <ref> omits <file>"`, `rationale` = the escalation's evidence). A second `cross-cut` on the same task goes to the user rather than widening again.
- **`spec-stale`** (either tier): the plan is wrong about the code. Mark the row `failed`, append its `task-completion` with `status = "skipped"` and the evidence as `summary`, and surface it now with a `/plan-update` recommendation; its dependents stay blocked.
- **Any other `implement-deep` escalation** (`ambiguous`, `security-sensitive`): the user's call — record and surface it as for `spec-stale`.
- **`stash-required`**: the stash escalation handler below.

A `/tdd` sub-flow has no row to write: widen its pasted `Files` in the re-dispatch prompt instead, and record the rest in `<record>` alone.

A **partial** return (`applied <id>: partial — <landed>; pending: <rest>`) is not a completion: re-dispatch the pending part to `implement-deep` on the same row, with what landed in the prompt. It spends one retry.

**Stash escalation handler (delegate-emitted `stash-required`).** Delegates may NOT run `git stash`/`reset`/`checkout --`; they return `escalate <id>: stash-required — what="<operation>" why="<reason>"` and exit. As orchestrator: (1) pause frontier dispatch and wait for all in-flight agents to terminate (concurrent stash is unsafe — strictly serial); (2) `git stash push -u -m "implement-escalation-stash-<ISO timestamp>"`, capture the stash ref (skip to step 4 if `No local changes to save`); (3) perform the requested observation (typically a `Read` of the now-clean on-disk state — no new edits); (4) `git stash pop <stash-ref>` — **if `git stash pop` reports a merge conflict, do NOT auto-resolve — surface the conflict to the user with the literal stash ref and halt the run (do not proceed to step 5); the stash remains on the stack for user recovery**; (5) re-dispatch the delegate with a "Stash escalation context" preamble (counts one retry attempt); (6) surface the event in the Phase-4 report. Never `git checkout --`/`restore` (discards work without recovery); re-derive working-tree state via `git status --porcelain`; a handler that cannot satisfy the escalation terminates the task `failed` — do not loop.

## Phase 3: Verify

Determine verification commands (Phase-1 extraction, else CLAUDE.md / project-root manifests; ask if ambiguous). This is the FINAL verification tier — each checkpoint (Phase 2 step 5) already gated the fast tier, so Phase 3 runs everything across all touched crates: build → tests → `e2e:` → the plan's `success:` commands, in the order it lists them → lint + audit, each with its `timeout:` / `rerun:` options, plus the plan's `transient:` patterns. A `success:` command checks a `## Success Criteria` criterion rather than running a suite, so its block is judged by exit status alone: `exit: 0` is `pass`, and the step-5(ii) test-count rule does not apply to it. A plan with no `success:` key runs the list without them. Launch the `verification` agent once (`subagent_type: "verification"`) with the full ordered `commands:` list; it stops at the first `fail` or `timeout`, and each block is judged by the step-5(ii) rules. Per command actually executed, append one `type=verification` entry to `<record>` through `flow record`, batched with `--ndjson` as in step 5(ii) (required `command`, `outcome ∈ {pass,fail,timeout,flaky}`, plus `duration_s` / `failed_ids` when the block carries them); the verb restamps `last_updated` itself. On `fail`: diagnose in main conversation, fix directly or via targeted agent (counts against budget — max 2 fix-and-reverify cycles), re-dispatch; `flaky` and `timeout` cost no cycle. A re-run for the same `(command, task_ref)` MUST set `supersedes_entry` to the prior verification id (query last id first). **Final commit train (milestones/single only):** after the full pass is green — every command `pass` or `flaky`, each judged by the step-5(ii) rules — commit any terminal work not yet committed as a step-5(iii) commit train — under `single` this is the run's only train (`commit_granularity: single-commit` yields exactly one commit), and it appends the same `kind = "commit-train"` checkpoint entry. Per-batch legacy mode keeps its existing convention: the final batch stays uncommitted for the user's post-review commit. **End of Phase 3:** regenerate `PROGRESS-LOG.md` by running `tomlctl flow render-progress-log --slug <slug>` (cheap, idempotent — guards against the Phase-4.5 no-op gate skipping the render). This regenerates the log deterministically as a pure function of `<record>` + the flow title; PROGRESS-LOG.md is DERIVED, so it carries no `.sha256` sidecar.

```bash
tomlctl flow render-progress-log --slug <slug>
```

## Phase 4: Report

**Reason thoroughly.** After successful verification, render the plan from the store, reconcile the backlog, harvest the backlog, then output the Implementation Summary.

**Plan render — `--check` first, always.** The store owns `## Execution Policy`, `## Tasks` and `## Dependency Graph`; every other byte of the plan document is preserved. Run the drift gate before anything writes:

```bash
tomlctl tasks render --slug <slug> --check
```

A clean `--check` means the render is a no-op — skip it. **Drift is not permission to overwrite.** `--check` exits `1` when the markdown and the store disagree, and the usual cause is hand edits the store lacks: a retitled heading, a task added by hand, an edited `Files` line. Rendering over them destroys them. Re-import instead, through the ref-set diff gate:

```bash
tomlctl tasks import-plan --slug <slug> --dry-run
```

Inspect `added_refs` / `removed_refs` and surface both. A non-empty `removed_refs` means a heading was renamed or a task deleted: **abort on any removed ref whose row is not `pending`** — that is a settled task about to be orphaned. A rename does not arrive as a clean add/remove pair on a real import, because the old row is never deleted and keeps its task number: the renamed heading arrives with a new `ref` and the same number, and the import refuses on `dag/duplicate-number`. Rename the row rather than the store (`tomlctl tasks update <id> --slug <slug> --ref <new-ref>`), then re-import. Only once the diff is understood, take the real import and the render:

```bash
tomlctl tasks import-plan --slug <slug>
tomlctl tasks render --slug <slug>
```

The render is a derived write like `PROGRESS-LOG.md` and carries no `.sha256` sidecar. It refuses — writing nothing — on a cycle or a dangling edge, because a marker naming the wrong tasks is worse than a refusal.

**Backlog reconcile.** Only when the Phase-3 final pass was green — never after a red or unfinished pass — resolve the backlog items this flow delivered:

```bash
tomlctl backlog reconcile --flow <slug> --apply --select applied,skipped
```

It resolves every `ready` item (every task that `closes` it is `done`), recording `resolved_flow`, `resolved_tasks` and `resolved_commits`. Under per-batch legacy the final batch is still uncommitted, so `resolved_commits` may be empty; that needs no action. Report `applied` as the resolved ids, and surface each `skipped` entry with its reason.

**Backlog harvest.** Invoke the `backlog-capture` skill to load the capture discipline (mint criteria, the verdict table, the fingerprint rule, the vocabularies, the check-then-add gate). Collect every `TANGENTIAL:` line the Phase-2 agents returned and every `TANGENTIAL: flaky-test` line a `flaky` verification block carried (checkpoint gates and Phase 3 alike), plus any out-of-scope discovery in the Failed / Skipped and Plan Deviations material — a plan deviation remains a `type=deviation` record; only what falls outside the plan's item set is a backlog candidate. Run the skill's gate on each candidate, minting with `--origin implement --flow <slug>`, and list what it minted or bumped on the report's `Backlog` line.

**Three dispositions, not two.** Every harvested discovery is exactly one of: a **plan deviation**, which stays a `type=deviation` record and is never a backlog candidate; a **same-run task**, minted per Phase 2 step 3c and scheduled through the normal frontier; or a **backlog item**, minted through the gate above. The disposition is the orchestrator's — a sub-agent surfaces a candidate on its `TANGENTIAL:` line and decides nothing. Deferral is the default for an out-of-plan discovery; a same-run task must clear all three of:

1. **Cost to fix now** — cheap in the sense of needing no research or design pass of its own. Anything you would have to investigate before you could write its `Action` is a backlog item.
2. **Blast radius** — the change stays inside a file this run already touched. A discovery reaching across files the run never opened is a backlog item however small each edit looks.
3. **Verification coverage** — the `## Verification Commands` this run already runs must actually exercise the touched file. Where they do not, a same-run task ships an unverified change and buys nothing over a backlog item, which at least records that nothing was checked.

Fewer than three ⇒ backlog item. Accepting one re-opens the run: mint it per step 3c (`tasks add`, `type=deviation` entry recording the mint, plan-behind-the-flow note in Next Steps), dispatch it, and re-enter Phase 3 — work landing after the final pass is unverified by definition, and that cost is why the bar is three factors rather than judgement. An accepted candidate is now in scope, so it is fixed rather than captured; do not also mint it into the backlog.

```
## Implementation Summary

### Completed
- [task] — files changed, what was done

### Failed / Skipped
- [task] — reason, what needs manual attention

### Plan Deviations
- [task] — plan assumption vs. what was found, how handled (adapted / deferred / reverted)

### Backlog
- [id] — verdict, summary; `(none)` when nothing was captured
- Resolved: [ids from `applied`]; remaining: in-progress N, stalled N, unlinked N, orphaned N, dangling N, external N

### Verification
- Build / Tests / Lint: pass/flaky/fail/timeout, each test line with its block's `summary:` count; fix attempts used: N/M
- Success criteria: each `success:` command with its outcome
### After Merge
- [the plan's `## After Merge` bullets, verbatim]
### Next Steps
- review the revised plan + PROGRESS-LOG.md; then /review + /optimise on the scope, then /plan-update <slug> complete
```

**After Merge** repeats the plan's `## After Merge` list verbatim, `_None._` included, because those are steps the user must take by hand once the work lands. Omit the heading when the plan has no such section, and omit the `Success criteria` line when it has no `success:` key.

### Phase 4.5: Sync plan context

1. **No-op gate:** if `[tasks].in_progress == 0` AND no scoped files were edited, skip and emit `Phase 4.5 skipped: no-op gate (in_progress=<N>, scoped-edits=<count>)`.
1a. **Reset the in-flight counter** — `tomlctl set <context_path> tasks.in_progress 0`. Phase 1 raised it and nothing else lowers it; without this the field stays non-zero forever, the gate above can never fire again for this flow, and a finished flow reports in-flight work it does not have.
2. **Otherwise** call the `plan-update` skill with literal arg `status` (`Skill("plan-update", "status")`); it refreshes `[tasks]` counters, sets `updated`, preserves `created`, re-renders `PROGRESS-LOG.md`, and MAY transition to `review` but MUST NOT transition to `complete`.
3. When `status` is now `review`, append the one-line hint `flow <slug>: implementation complete — status is now "review". Run /review and /optimise against the scope, then /plan-update <slug> complete to drop the flow from auto-resolution.` (interpolate `<slug>`).

## Important Constraints

- **Context budget** — be selective in Phase 1; agents read their own targets. **Front-load complex analysis** — give agents pre-digested instructions, not open-ended problems.
- **Up to `max_parallel` implementation agents in flight (the store's `[policy]`; default 6, ceiling 8)**; file ownership is absolute (no two in-flight agents touch one file — `tasks ready` enforces it by holding the claimant; serialise via a dependency edge if needed). **Commit cadence follows the execution policy** (absent ⇒ per-batch legacy); **staging is always selective** — `git add -- <validated files>`, never `-A`/`.`; preserve existing patterns; do not over-implement.
- **Verification is orchestrator-owned, never per sub-agent, never inline** — it runs through the `verification` agent at two tiers: a per-checkpoint gate (the fast tier — the plan's `checkpoint:` commands, else build + the touched crate's tests — blocking each checkpoint commit) and the final full pass (build + tests + e2e + lint + audit). Delegates do not self-verify with whole-suite builds/tests, and the orchestrator runs no build or suite in the main conversation; never report success without the final pass. **Retry budget is strict** — max 2 fix attempts per task failure, max 2 fix-and-reverify cycles for verification; a `flaky` or `timeout` block consumes neither.
- **Plan deviations surface immediately** — agents report mismatches; the orchestrator decides proceed/fix/abort.
