---
description: Implement a plan or task using parallel sub-agents with research, progress tracking, and verification
argument-hint: [--flow <slug>] [plan path or task description]
---

# Implementation

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

Implements a plan, feature, or task by delegating to parallel sub-agents — work decomposition, research for novel steps, efficient parallelisation, progress tracking via Task tools, and verification. Accepts plan files, plan directories, specific items (`items 3,4,5 from …`), inline task descriptions, or no arguments (auto-resolves the active flow via the Step-0 envelope).

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

**Degradation is a halt, never a fallback.** Halt the run with a named diagnostic when `tasks check` reports an error-class finding, when the store is still absent after an import that claimed to succeed, or when `tomlctl tasks` is unrecognised (a stale binary on `PATH`). The diagnostic names both recoveries: `cargo install --path tomlctl` for the stale binary, and a `git revert` of the four carrier adoptions (`/plan-new`, `/implement`, `/review-plan`, `/plan-update`) to fall back to the prose frontier. Never proceed on partial store output — a frontier computed from a store that failed its own checks dispatches the wrong tasks in the wrong order, and it does so silently.

## Phase 1: Analyse and Decompose (main conversation — thinking enabled)

**Reason thoroughly.** Front-load analysis here. Read the resolved flow's `context.toml`, extract `plan_path`, and read the plan.

**Plan-path validation (mandatory, before any plan Read).** The plan's narrative sections are embedded verbatim into every Phase-2 agent prompt's shared preamble. Resolve the candidate path and verify it falls under the git top-level. **Reject** if: (1) it contains `..` after normalisation; (2) it is absolute and not under the git top-level prefix (`/tmp`, `/etc`, `~/`, etc.); (3) it resolves outside the repo via symlink. Halt naming the offending path; dispatch no agent. This binds to the initial read, outline/detail reads, and any Phase-4.5 re-read.

Handle plan-directory (start at outline/master, then only relevant detail docs), single-file, inline-task, and item-subset (`items 3,4,5`) inputs. Update the resolved `context.toml`: read pre-update `status` as `<old_status>`, set `status = "in-progress"`, set `updated` to today, set `[tasks].total` to the store's row count (`tomlctl tasks list --slug <slug> --count`), set `[tasks].in_progress` to the count of tasks this run will dispatch, preserve `created` and key order. **`[tasks].in_progress` is derived from the frontier scheduler's in-flight set, never from the task surface** — the task tools may be absent entirely (see the task-visibility contract), and a persisted field that mirrors them would diverge silently. It is written only by `/implement`; `/plan-new` and `/plan-update` never touch it, and Phase 4.5 resets it.

Invoke the `flow-contract-execution-record-schema` skill to load the canonical execution-record contract (schema, type vocabulary + per-type required fields, the two-call heredoc write contract, `<record>` path-resolution rule, `[[items]]` subcommand restrictions, append-only/supersession, deterministic PROGRESS-LOG.md regeneration via `tomlctl flow render-progress-log`, `[tasks].completed` derivation, read-path `--verify-integrity` integrity contract, field-length caps, and read rules). Resolve `<record>` as `[artifacts].execution_record` (fallback `.claude/flows/<slug>/execution-record.toml`); no explicit pre-create is needed — `flow init` / `/plan-new` normally pre-seed it, and if it is absent on disk the first mutating `tomlctl items add` / `set` against `<record>` auto-creates and seeds it with the byte-identical `schema_version = 1` + `last_updated = <today>` skeleton (the write success envelope then carries `"created": true`). Use `<record>` (fully-qualified) for every later `tomlctl` call — never the bare filename. If `<old_status> != "in-progress"`, append a `type=status-transition` entry per the skill's heredoc form (skip the no-op case). Build the idempotency skip-list from the store — `tomlctl tasks list --slug <slug> --where status=done --pluck ref` — and skip every task whose `ref` it names. **A record `task_ref` is the store row's `ref`**: the execution-record contract pins the two to one value, which is what lets Step 0.5's `--reconcile-record` import carry a prior run's completions into row statuses, and what makes this one query equivalent to the record scan it replaces. A `/tdd` sub-flow, having no store, keeps that scan: `tomlctl items list <record> --where type=task-completion --where status=done --pluck task_ref --verify-integrity`. Extract `## Verification Commands` for Phase 3.

Research novel/complex steps now (Context7 + WebSearch), resolve ambiguities, and classify each task Straightforward vs Complex. **The DAG, the execution policy and the checkpoint groups are read from the store, not re-derived from the plan prose** — the plan's `## Tasks`, `## Execution Policy` and `## Dependency Graph` sections are rendered from it. Gate on `check` before binding anything, then bind `checkpoint_policy ∈ {single, milestones, per-batch}`, `max_parallel` (default 6, ceiling 8), and `commit_granularity ∈ {per-task, per-checkpoint, single-commit}` (default per-task) from `[policy]`, and the checkpoint groups from `[[checkpoints]]`:

```bash
tomlctl tasks check --slug <slug>
tomlctl get <store> policy
tomlctl tasks batches --slug <slug>
```

Any error-class finding halts per Step 0.5's degradation rule; warnings are surfaced and carried into the Phase-4 report. `dag/unreachable-claim` is the one to read closely — it names the file-claim collisions the frontier will hold on at dispatch. **A `/tdd` sub-flow, or a legacy flow whose plan carries no `## Execution Policy`, binds legacy mode**: per-batch cadence, per-checkpoint granularity, `max_parallel` 6. Only `max_parallel` coincides with the store's own fallback — an absent or partial `[policy]` table falls back to milestones / 6 / per-task — so legacy mode is bound *in place of* the three stored values, never read from them. **The discriminator is `[policy].origin`** in the table the `tomlctl get` above already returned, because the import always materialises a `[policy]` table and a pre-policy plan is otherwise indistinguishable afterwards: a plan carrying no `## Execution Policy` imports as `origin = "default"` — every field a house default, and a `plan/policy-absent` warning raised at the import that substituted them — while `origin = "plan"` means the plan authored the section and `[policy]` is read as-is. **Never test `[policy].note` for mode**: it is authored prose, and an absent policy section leaves it empty. A `/tdd` sub-flow has no store at all and binds legacy mode without the check.

Invoke the `flow-contract-task-visibility` skill to load the run-scoped task-surface contract (view-not-store rule, the `<slug> /<command> · <ref> — <title>` subject prefix, create-up-front lifecycle, `blockedBy` DAG mirroring, granularity floor, and the mandatory silent-degradation rule). Mint the full task set for the run now — one entry per plan task **not already on the idempotency skip-list**, subject prefix `<slug> /implement · t<n>` with `activeForm` set, `addBlockedBy` mirroring the DAG — before dispatching any of Phase 2. Minting skip-listed tasks would leave a resumed flow showing rows that are never dispatched and never transitioned.

## Phase 2: Execute (parallel sub-agents)

**Frontier scheduling.** The frontier is a verb, not a re-derivation. Pass the ids of every dispatched, non-terminal task as `--in-flight`:

```bash
tomlctl tasks ready --slug <slug> --in-flight 3,4
```

It answers `{ready[], held[{id, blocked_on_file, holder}], next[], blocked[{id, blocker, blocker_status}]}`. **The file-claim check is inside the verb:** `ready` already excludes every row sharing a file with an in-flight one, so it is directly dispatchable, and `held` names each excluded row with the file and the holder that blocks it. Two ready tasks sharing a file run one after the other, never together — pass the ids being dispatched in this same response into the next `--in-flight` before asking again. A `held` entry reveals a missing dependency edge in the plan: do not silently mint the edge — record it and surface it in the Phase 4 report under `### Plan Deviations` (assumption: tasks independent; found: file overlap, serialised at dispatch). Dispatch every id in `ready`, up to `max_parallel` in flight — **all currently-ready tasks MUST be emitted in the same assistant response**: N ids in `ready` ⇒ N `Agent` blocks before the turn ends, no fewer; deferring one to a later turn serialises it for no reason. Do NOT reduce the agent count. As each agent completes, process its return (steps 3a/3b/5b below), move its row to a terminal status, re-run `tasks ready` with the shrunken `--in-flight` set, and dispatch newly-ready tasks **immediately in the same turn** — never hold a ready task waiting for unrelated in-flight work. The only legitimate pause is a checkpoint drain (step 5). Under per-batch legacy mode this degrades to the classic dependency-level batches — `tomlctl tasks batches --slug <slug>` gives the levels, each fully dispatched in one response, gate + commit between levels. Place shared context (the plan's narrative sections + common constraints, never its `## Tasks` prose — the store owns that) as a byte-identical literal preamble atop every agent prompt for the whole flow (prompt-cache reuse), with per-agent divergence below a divider.

**An exhausted frontier is not completion.** `blocked` names the rows no later wave can reach — each with the **nearest ancestor** that is neither `done`, nor `pending`, nor in the `--in-flight` set just passed. The walk climbs through intervening `pending` rows, so `blocker` is always the stall itself and never a row between: the id it names is the one to act on, and one stall names itself once per pending row behind it. All four keys empty means the run is finished; `ready`, `held` and `next` empty while `blocked` is not is a **stall**, and treating it as completion ends the run silently short of the plan. Read each entry's `blocker_status` before deciding: `failed` is the expected shape after a step-6 rollback, and those dependents stay blocked by design. `in-progress` on a `blocker` the `--in-flight` set does not name is a **crashed dispatch** — no agent holds that blocker, so nothing will ever move it and the wait is unbounded. Route the blocker to the resume path rather than waiting on it: settle it `done` if `<record>` holds its `task-completion`, else re-dispatch it as an unstarted task. A declared in-flight ancestor never stalls, so a task genuinely being worked keeps its dependents in `next`.

**Agent dispatch rules — fetch by id, not paste.** Below the byte-identical preamble, a prompt carries the task's **id** and the command that fetches its own body:

```bash
tomlctl tasks show <id> --slug <slug> --with body,files,deps
```

That is the store's whole point: `## Tasks` is the largest single thing an orchestrator loads, and only the agent executing a task needs that task's prose. `--with deps` gives it the direct dependency summaries it needs to know what already exists; `--with dependents` is a planning read, not a dispatch read, and grows with the plan. **A sub-agent gets the read verbs only** — `show`, `list`, `edges`, `ready`, `batches`, `closure`, `check`. Every store write is the orchestrator's: two writers means a row's status and the record's `task-completion` entries can disagree, and the store has no supersession mechanism to reconcile them.

Beyond the id and that command, each prompt MUST include: exact files (absolute paths); "read every listed file in full plus any file you import/export"; what the code should do and why; research findings for complex tasks; specific API signatures; success criteria; mandatory Context7/WebSearch verification; step-by-step reasoning; and the plan-deviation protocol ("if the plan's assumptions are wrong, do NOT silently improvise — complete unaffected changes, report what was assumed vs found vs left undone"). Include Context7/WebSearch/codebase-exploration/diagnostics tool guidance tailored per task. **No whole-suite self-verification:** each prompt MUST also direct the agent NOT to run the full build or whole test suite to self-check (`cargo build` / a bare `cargo test` / `cargo nextest` / `bun run build`) — those are the orchestrator's job at the batch checkpoint (Phase 2 step 5) and the final pass (Phase 3). If the task's `Acceptance` names a whole-suite command, the agent treats it as the orchestrator's checkpoint responsibility, not its own; a delegate may still run a cheap check (`cargo check` / `cargo clippy` / `bun run type-check`) or its task's own narrow test (`cargo test --test <name>` / `cargo nextest -E 'test(<area>)'`) before handoff.

**Falsifier check (every prompt).** Each prompt MUST also direct: *for every assertion you add or change, confirm it actually fails against the pre-change behaviour* — restore the old constant, comment out the new line, or feed the input the assertion is meant to reject — then observe the failure, report the observed failure count, and restore. An assertion that passes both before and after your change is guarding nothing, and both sides of a comparison being `undefined` is the common shape. Do this **in-editor only: never `git stash` / `reset` / `checkout --` / `restore`** — those are orchestrator-only (see the stash-escalation handler), and siblings are editing the same tree with up to `max_parallel` agents in flight. **Do NOT emit `escalate <id>: stash-required` for this check**: retype the prior value from your own edit, which you already know — you do not need the on-disk pre-edit state, and that escalation pauses frontier dispatch until every in-flight agent terminates. Agents that volunteered this check have historically caught real vacuous assertions that every declared gate passed.

**Lite-eligibility gate (per-task dispatch).** Pick `implement-deep` (default) or `implement-lite` **per task**. Effort tag is primary: `S` is lite-eligible subject to the criteria; `M`/`L` ALWAYS go deep (do not evaluate the rest); no/unknown tag ⇒ treat as `M`. The four criteria (S only): (1) ≤ 2 files; (2) action fully specified, no design decisions left; (3) no cross-file refactor; (4) not security-sensitive (auth/crypto/input-validation/sandbox/token-storage). Coupling-isolation: when tasks are dispatched as one coupled cluster (one coherent change split across prompts), a single failing task sends the whole cluster deep — do not peel tasks out of a cluster; independent file-disjoint tasks that merely share a response are NOT a cluster and gate individually. Record the choice as a one-line `DISPATCH:` header atop each agent prompt.

For each dispatch: move each dispatched row to `in-progress` and record the tier's agent on it, then TaskUpdate the dispatched tasks to `in_progress` (re-check the all-ready-tasks-emitted invariant before ending the turn). The store write is what makes the next `tasks ready` correct — a row left `pending` while its agent runs is offered again.

```bash
tomlctl tasks update <id> --slug <slug> --status in-progress --agent implement-deep
```

Per completed agent:

- **3a. Vet `implement-lite` output before promoting completion.** Inspect every `applied <id> [vet-recommended]` tag (read touched files, confirm soundness); spot-sample ≥ 1 bare `applied` per lite cluster (confirm match to `Action`+`Detail`, style preserved). Sample-failure ⇒ expand to 100% of that cluster and re-dispatch failed items to `implement-deep` (counts as a failed retry). Skip vetting for deep clusters. Do NOT append a `task-completion` for a task whose vet failed.
- **3b. Plan deviations.** Per detected deviation, append a `type=deviation` entry to `<record>` (skill heredoc form; required `original_intent`, `rationale`, `commits`). **Pre-append dedupe guard (mid-batch-crash safety):** query for an existing match on the `(task_ref, original_intent, rationale)` triple via `tomlctl items list <record> --where type=deviation … --count`; append only when count is 0 (or dedupe on a `deviation_fingerprint` hash). Significant deviations pause and surface to the user. Do NOT advise a second `/plan-update deviation` for an already-recorded deviation.
- **3c. Minting a task the plan lacks.** When execution reveals necessary work no task owns (a repair for a contradiction between two acceptances, a call site for a seam, a mitigation the Risks section describes but assigns to nobody), mint it into the store rather than smuggling it into an adjacent task's diff. `tasks add` mints the id and the `ref`, and refuses a dangling dependency target or a cycle before writing anything:

```bash
tomlctl tasks add --slug <slug> --title "<title>" --effort S --files <f1>,<f2> --needs <ids> --checkpoint <group> --action "<what to do>" --acceptance "<how it is verified>"
```

The returned `batch` is the Kahn round the row lands in; the next `tasks ready` schedules it through the normal frontier. Give it the checkpoint group whose closure it belongs in — usually the one holding the task that forced the mint — and pass a group `[[checkpoints]]` actually declares: `add` refuses an undeclared id outright, naming the ones the store holds, so a mistyped group costs a re-issued command rather than a row nothing commits. Omitting `--checkpoint` is the legal "no group" spelling, and such a row is committed only by the Phase-3 train, which `tasks check` reports as `checkpoint/orphan-task`. Append a `type=deviation` entry recording the mint (`original_intent = "plan carries no task for <X>"`, `rationale` = what forced it). Then re-read `[tasks].total` from `tomlctl tasks list --slug <slug> --count` and write it to `context.toml`. **The plan document is behind the flow until Phase 4 renders it**: the render writes the minted task into `## Tasks` from the store, so name the divergence in Phase 4's Next Steps (`plan document described N tasks; N+K executed`) and confirm the render carried it — recommend `/plan-update reformat` only where the render was skipped or refused. Never silently widen a sibling task's scope to absorb the work — that destroys the file-claim invariant the scheduler depends on.
- **4.** Move each returned row to its terminal store status — `--status done` on success, `--status failed` once the retry budget is spent — then TaskUpdate completed tasks to `completed`. The two vocabularies are separate on purpose: a store row settles as `done` or `failed`, while both go `completed` in the task view. Failed and deviation tasks ALSO go `completed`, with the failure named in `description` — never left `in_progress`, which a reader cannot tell apart from still-running.

  ```bash
  tomlctl tasks update <id> --slug <slug> --status done
  ```

  Execution continues (dependents stay blocked). Before `git commit`, apply the `commit-conventions` skill if installed and emit the sentinel `IMPLEMENT-AUTOCOMMIT: phase-2-step-5`.
- **5. Checkpoint: drain → gate → commit train.** Checkpoints fire per the bound policy: `per-batch` — after each dependency level that has dependents (legacy behaviour; if no dependent batch follows, skip gate and commit — the Phase-3 pass covers it); `milestones` — when every member of a declared checkpoint group is terminal, the membership being the verb's answer rather than a prose closure; `single` — never mid-run (Phase 3 is the only gate; step 5b entries carry empty `commits[]` until the final train). At a checkpoint:

  ```bash
  tomlctl tasks closure --slug <slug> --checkpoint A
  ```

  `members[]` is the group, `maximal[]` its `Checkpoint after` antichain, and `valid_cut` says whether the group is committable *here* — it covers the union with every earlier group, so `false` means the prefix is not downward-closed and the window would commit a task whose dependencies are still open. Surface a `false` as a plan defect and do not commit against it.

  - **(i) Drain.** Stop dispatching new tasks and let in-flight agents finish. The gate needs a quiescent tree — never build/test while agents are editing.
  - **(ii) Gate.** Dispatch the `verification` agent (`subagent_type: "verification"`, Haiku) with the checkpoint command list: for each crate the window touched, build it then run its full test suite — `cargo build --manifest-path <crate>/Cargo.toml` then `cargo nextest run --manifest-path <crate>/Cargo.toml` (or `cargo test`); for SPA-touching windows, `bun run build` then `bun test`. It short-circuits on first `fail`. On `fail`: diagnose in the main conversation and fix directly or via a targeted agent (counts against the retry budget — max 2 fix-and-reverify cycles), then re-run; **do NOT commit a red window** — if it cannot go green within budget, go to step 6. On green: **corroborate before trusting** — Haiku verification has historically reported a pass while a test binary failed. Use the short-circuit property (commands run sequentially, stopping on the first `fail`, so a later command having executed proves every earlier one passed) and scrutinise the FINAL command, which has no later witness: if its reported outcome or tail is at all ambiguous, re-run it (or its narrowest per-binary equivalent, e.g. `cargo test --test <name>`) directly before committing. **Treat a reported pass whose output shows zero tests executed as a `fail`** (`No test files found`, `0 passed`, `no tests to run`): a path filter matching nothing exits 0 in most runners, so a mis-rooted or mistyped acceptance command reads green on a suite that never ran. This judgement is the orchestrator's — leave the `verification` agent's pass|fail-per-command, no-interpretation contract alone. Only then append one `type=verification` entry per executed command (skill heredoc form; `command`, `outcome`; `task_ref` = the checkpoint's lead task).
  - **(iii) Commit train (selective staging).** Partition the window's completed tasks into commit groups per `commit_granularity` (default one commit per task; `per-checkpoint` ⇒ one commit for the window); **merge any groups whose `files[]` overlap** — their edits are no longer separable in the tree. Per group, in dependency order: apply the `commit-conventions` skill, emit the sentinel `IMPLEMENT-AUTOCOMMIT: phase-2-step-5`, stage EXACTLY the union of the group's validated `files[]` (`git add -- <path>...` — NEVER `git add -A` / `git add .` / `commit -a`), and commit. Dirty paths claimed by no completed task are never committed: files owned by still-pending tasks are expected and left alone; unexplained leftovers halt the train for user review. Only the train's tip is gate-verified — intermediate commits are logical splits of a verified tree, attributable via the execution record's SHA→task_refs map. **If any `git commit` fails (e.g. a pre-commit hook rejects it): halt the train, surface the failure to the user, and do not record that SHA against any task.** After each group commits, record its SHA on the group's rows (`tomlctl tasks update <id> --slug <slug> --commit <sha>`). After the train, append one `type=checkpoint` entry (`kind = "commit-train"`, `commits = [<train SHAs>]`, `summary` mapping each SHA to its task_refs), then resume frontier dispatch.
- **5b.** Per terminal-state task, append a `type=task-completion` entry **at completion time** (crash-safe — Step 0.5's `--reconcile-record` reads these entries on the next resume to repair a row a crash left non-terminal, so an entry batched to the checkpoint is one a crash loses; skill heredoc form). **`task_ref` is the store row's `ref`** — take it from the row (`tomlctl tasks show <id> --slug <slug>` emits it) rather than re-deriving it from the heading; the execution-record contract pins the two to one value, and that join is what Step 0.5's `--reconcile-record` import and Phase 1's skip-list both stand on. Required: `task_ref`, `status ∈ {done,failed,skipped}`, `files` (verbatim from agent, after the filter below), `commits` (the real SHA when the task's commit already exists at append time — per-batch mode commits in step 5 first, so its entries carry the SHA as before; under milestones/single, `[]` — the subsequent `kind = "commit-train"` checkpoint entry is the authoritative SHA→task_refs map), `dispatch_tier ∈ {lite,deep}`, `dispatch_agent ∈ {implement-lite,implement-deep}` (tier↔agent invariant). **Path validation for `files[]` (before the add):** reject entries beginning with `/`, `\\`, or `~` and any with `..` components — drop them with a console warning; if the array empties, halt with `"Phase 2 step 5b refused to persist task-completion for <task_ref> because all reported files[] failed validation"` and append nothing (rerun picks it up via the skip-list). Out-of-`scope` paths get a soft warning + `scope_warning = true`, not a reject. **Free-text JSON sanitisation:** RFC-8259-encode every agent-supplied free-text field (`task_ref`, `summary`, `rationale`, `original_intent`, `reason`, `reevaluate_when`) — escape `\\`, `"`, U+0000–U+001F, U+2028, U+2029 — for every JSON-payload heredoc in `/implement`. Conclude every two-call write with `tomlctl set <record> last_updated <today>`.
- **6. Rollback on failure.** Re-dispatch the failed task to `implement-deep` FIRST (counts against the retry budget) — never roll back sibling tasks' successful work. Only after the budget is exhausted: for **uncommitted work** (a milestones/single window, or a task that never reached a commit), restore ONLY the failed task's declared files to HEAD (`git restore -- <files>` — safe because file ownership is absolute; never touch files owned by other tasks; if the failed task's file set is uncertain, stash first and surface), mark the row `failed` per step 4, keep its dependents blocked — `tasks ready` does that on its own, since a dependent's in-degree only clears on `done` — and continue the frontier; for **committed work** (a red checkpoint already landed, per-batch mode), `git revert` scoped to the failing items' files — successful items retain changes.

**Retry budget:** max 2 fix attempts per failure; after that mark failed, revert if it breaks the build, continue. **Cross-cutting changes:** give a 15-file rename to one agent (never split); if too large, sequence (definition+direct consumers, then indirect).

**Stash escalation handler (delegate-emitted `stash-required`).** Delegates may NOT run `git stash`/`reset`/`checkout --`; they return `escalate <id>: stash-required — <reason>` and exit. As orchestrator: (1) pause frontier dispatch and wait for all in-flight agents to terminate (concurrent stash is unsafe — strictly serial); (2) `git stash push -u -m "implement-escalation-stash-<ISO timestamp>"`, capture the stash ref (skip to step 4 if `No local changes to save`); (3) perform the requested observation (typically a `Read` of the now-clean on-disk state — no new edits); (4) `git stash pop <stash-ref>` — **if `git stash pop` reports a merge conflict, do NOT auto-resolve — surface the conflict to the user with the literal stash ref and halt the run (do not proceed to step 5); the stash remains on the stack for user recovery**; (5) re-dispatch the delegate with a "Stash escalation context" preamble (counts one retry attempt); (6) surface the event in the Phase-4 report. Never `git checkout --`/`restore` (discards work without recovery); re-derive working-tree state via `git status --porcelain`; a handler that cannot satisfy the escalation terminates the task `failed` — do not loop.

## Phase 3: Verify

Determine verification commands (Phase-1 extraction, else CLAUDE.md / project-root manifests; ask if ambiguous). This is the FINAL verification tier — each checkpoint (Phase 2 step 5) already gated build + the touched crates' tests, so Phase 3 runs the full ordered suite across all touched crates plus lint + audit. Launch the `verification` agent once (`subagent_type: "verification"`, Haiku) with the full ordered `commands:` list (build → tests → lint); it short-circuits on first `fail`. Per command actually executed, append one `type=verification` entry to `<record>` (skill heredoc form; required `command`, `outcome ∈ {pass,fail}`); conclude with one `tomlctl set <record> last_updated <today>`. On failure: diagnose in main conversation, fix directly or via targeted agent (counts against budget — max 2 fix-and-reverify cycles), re-run. A re-run for the same `(command, task_ref)` MUST set `supersedes_entry` to the prior verification id (query last id first). **Final commit train (milestones/single only):** after the full pass is green — corroborated per the step-5(ii) rule (short-circuit witness + scrutiny of the final, witness-less command) — commit any terminal work not yet committed as a step-5(iii) commit train — under `single` this is the run's only train (`commit_granularity: single-commit` yields exactly one commit), and it appends the same `kind = "commit-train"` checkpoint entry. Per-batch legacy mode keeps its existing convention: the final batch stays uncommitted for the user's post-review commit. **End of Phase 3:** regenerate `PROGRESS-LOG.md` by running `tomlctl flow render-progress-log --slug <slug>` (cheap, idempotent — guards against the Phase-4.5 no-op gate skipping the render). This regenerates the log deterministically as a pure function of `<record>` + the flow title; PROGRESS-LOG.md is DERIVED, so it carries no `.sha256` sidecar.

```bash
tomlctl flow render-progress-log --slug <slug>
```

## Phase 4: Report

**Reason thoroughly.** After successful verification, render the plan from the store, harvest the backlog, then output the Implementation Summary.

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

**Backlog harvest.** Invoke the `backlog-capture` skill to load the capture discipline (mint criteria, the verdict table, the fingerprint rule, the vocabularies, the check-then-add gate). Collect every `TANGENTIAL:` line the Phase-2 agents returned, plus any out-of-scope discovery in the Failed / Skipped and Plan Deviations material — a plan deviation remains a `type=deviation` record; only what falls outside the plan's item set is a backlog candidate. Run the skill's gate on each candidate, minting with `--origin implement --flow <slug>`, and list what it minted or bumped on the report's `Backlog` line.

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

### Verification
- Build / Tests / Lint: pass/fail; fix attempts used: N/M
### Next Steps
- review the revised plan + PROGRESS-LOG.md; then /review + /optimise on the scope, then /plan-update <slug> complete
```

### Phase 4.5: Sync plan context

1. **No-op gate:** if `[tasks].in_progress == 0` AND no scoped files were edited, skip and emit `Phase 4.5 skipped: no-op gate (in_progress=<N>, scoped-edits=<count>)`.
1a. **Reset the in-flight counter** — `tomlctl set <context_path> tasks.in_progress 0`. Phase 1 raised it and nothing else lowers it; without this the field stays non-zero forever, the gate above can never fire again for this flow, and a finished flow reports in-flight work it does not have.
2. **Otherwise** call the `plan-update` skill with literal arg `status` (`Skill("plan-update", "status")`); it refreshes `[tasks]` counters, sets `updated`, preserves `created`, re-renders `PROGRESS-LOG.md`, and MAY transition to `review` but MUST NOT transition to `complete`.
3. When `status` is now `review`, append the one-line hint `flow <slug>: implementation complete — status is now "review". Run /review and /optimise against the scope, then /plan-update <slug> complete to drop the flow from auto-resolution.` (interpolate `<slug>`).

## Important Constraints

- **Context budget** — be selective in Phase 1; agents read their own targets. **Front-load complex analysis** — give agents pre-digested instructions, not open-ended problems.
- **Up to `max_parallel` implementation agents in flight (the store's `[policy]`; default 6, ceiling 8)**; file ownership is absolute (no two in-flight agents touch one file — `tasks ready` enforces it by holding the claimant; serialise via a dependency edge if needed). **Commit cadence follows the execution policy** (absent ⇒ per-batch legacy); **staging is always selective** — `git add -- <validated files>`, never `-A`/`.`; preserve existing patterns; do not over-implement.
- **Verification is orchestrator-owned, never per sub-agent** — it runs at two tiers: a per-checkpoint gate (build + the touched crate's tests, blocking each checkpoint commit) and the final full pass (build + tests + lint + audit). Delegates do not self-verify with whole-suite builds/tests; never report success without the final pass. **Retry budget is strict** — max 2 fix attempts per task failure, max 2 fix-and-reverify cycles for verification.
- **Plan deviations surface immediately** — agents report mismatches; the orchestrator decides proceed/fix/abort.
