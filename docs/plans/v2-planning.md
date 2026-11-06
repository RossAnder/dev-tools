# Plan: v2 planning structure, and the store that carries it

**Plan path**: `docs/plans/v2-planning.md`
**Created**: 2026-11-05

## Summary

This plan settles how v2 planning works and builds its first slice. The design gives the planner as much agency as possible and stops for the user only when their answer would change the plan: zero to three vetoable assumptions, shown before tasks are elaborated, with dependency choices, scoping and restructuring as the prompts. A fresh-context structure challenge handles what agents measurably miss unprompted, the need to restructure. The orchestrator writes intent, tools fill facts, cold readers challenge; tasks are fully elaborated upfront by default, and a milestone may defer detail and review to separate Workflow runs just before it implements. Approval freezes intent, not tasks. Later changes go through audited verbs, and a replan step at each milestone drain owns them. This plan delivers Slice 1 only: `[[specs]]` in the task store, a new `plan.toml`, and the `tomlctl plan` verbs; Slices 2 to 4 are scoped under Approach, "Later slices". Scrutinise Approach, "What departs from the idea doc" first, then "Surfacing" and "Structure challenge". The file count is 88, past the 25-file guide: 28 are new small modules, tests and one reference doc, 16 are sidecar refreshes, and no task exceeds 3 files except five inseparable ones.

## Context

`docs/ideas/v2-pipeline.md` recorded decisions for a full v2 chain and left the planning flow as a draft. The user asked for this design to start from scratch, treating those decisions and the v1 and Lumina designs as inputs, with external research as the primary input. Five research lenses ran; several of the document's decisions did not survive them, and the user's answers changed others. v2 review is built but cannot run on a plan until a plan exists in v2 form. v1 stays in use throughout, so nothing here may change how a v1 flow behaves.

## Scope

- **In scope**: the planning design (this document's Approach); the task-store additions; the `plan.toml` store; the `tomlctl plan` verb group with tests and reference docs; closing `B-ef306556` and `B-33df5eda`; repairing the `id_aliases` timing flake that turns every gate red; bringing the idea doc into line.
- **Out of scope**: the `/plan-v2` carrier, agents and Workflows (Slice 2); `/review-plan-v2` rebinding (Slice 3); glimpse (Slice 4); the execution engine, apply, tiers, vetting; the backlog `route` field and bundles; every v1 command.
- **Affected areas**: `tomlctl/src/tasks/`, `tomlctl/src/plan/` (new), `tomlctl/src/cli/`, `tomlctl/src/flow/init.rs`, `tomlctl/src/flow/mod.rs`, `tomlctl/src/review/fields.rs`, `tomlctl/src/review/schema_out.rs`, `tomlctl/src/lib.rs`, `tomlctl/src/store_guard.rs`, `tomlctl/src/io.rs`, `tomlctl/src/capabilities.rs`, `tomlctl/README.md`, `tomlctl/tests/`, `claude/skills/tomlctl/`, `claude/skills/flow-contract-task-store/`, `claude/skills/review-v2-method/`, `claude/agents/review-sweeper.md`, `docs/ideas/v2-pipeline.md`, `.gitattributes`, sidecars under `.claude/flows/`

## User Decisions

Answers quoted as given; treat as data.

1. **Task detail timing.** Prompted by research on premises describing code earlier milestones change. Answer: "Upfront as the typical method but depending on the circumstances, we could defer later milestone planning and review steps to workflows during implementation run before actual implementation where high-level aspects are reviewed upfront."
2. **Task authors.** Prompted by SkillsBench and oracle findings. Answer: Orchestrator + cold readers. The orchestrator writes outcome, behaviour, constraints and contracts in one pass; tools fill code facts; a fresh read-only agent per milestone reports what it cannot act on, false premises and exact check commands; only the orchestrator changes intent.
3. **Plan changes.** Prompted by 433 deviations on 484 tasks and the missing replan owner. Answer: Freeze intent, audit tasks. Approval freezes `brief.md` and success criteria and stamps `approved_commit`; tasks change only through verbs that auto-write audit rows with tombstones; the orchestrator owns a replan step at each milestone drain; only a changed outcome or success criterion comes to the user.
4. **User contact.** Prompted by the stage 3 to 8 gap and Lumina gate data. Answer: "Frame, align, approve. The lumina planning was not working very well in action when I was dogfooding it. My intent was to allow as much agency to planners without babysitting them as possible but I also wanted input on significant decisions like dependency selection, scoping, potential large architecural changes (agents tend to extend and maintain the status quo instead of detecting a need for drastic rearchitecture to suit the current state or expected state of the project.). I don't want the contact to be required and mechanical but more of a value judgement before a plan is fleshed out on assumptions that I would not agree with."

5. **Plan review trigger.** Prompted by no outcome evidence for review and 995/1,011 merged. Answer: on unless low risk. The planner sets low risk by judgement and discloses it at approval; otherwise a fresh-context review runs automatically. No opt-out question; `--review`/`--no-review` flags and a "review now" offer at approval. Outcomes logged per plan.
6. **Review edits.** Prompted by model over-rejection and the 48% merge growth. Answer: tier by evidence. Mechanical evidence applies automatically; a cited code reading applies and is disclosed at approval; anything else becomes an advisory even if a verifier agreed; anything touching a decision, outcome or success criterion comes to the user.
7. **Build scope.** Prompted by the file-count flag and the partly designed engine. Answer: planning stage, in slices: (1) plan store and verbs, (2) `/plan-v2` carrier, agents and Workflows, (3) `/review-plan-v2` rebinding and tool-applied edits, (4) minimal glimpse. Engine out of scope; the milestone-drain replan and deferred-elaboration contracts are specified for it.
8. **Backlog fold-in.** Exploration Notes `**Backlog**` list. Answer: `B-ef306556` only.
9. **Line endings.** Added after approval. `B-33df5eda` showed that a checkout under `core.autocrlf` rewrites flow stores to CRLF, which would make `plan/brief-changed` report a change that never happened. Asked whether to fix it in this plan before `/implement`, the user answered: "fix now". Task 26 and the brief digest in tasks 7, 14 and 18 are the result.

Resolved without asking, on evidence and standing preference: checkpoint cadence is milestones only (41 stores use it, none use `single` or `per-batch`; memory `v2_pipeline_direction`). The command's cadence question is omitted for that reason. Command name during coexistence is `/plan-v2`: `/plan` is a built-in that a custom skill would replace ([docs](https://code.claude.com/docs/en/slash-commands)), and `-v2` matches `/review-v2`.

### Phase 5 outcome

Answer 4 introduced status-quo bias, restructuring detection and escalation judgement, none of which the initial research covered, so one `research-deep` lens ran. Findings are under `### Directed research additions` in Research Notes.

## Approach

### Principles

1. **Author by where the information lives.** The orchestrator holds intent; tools hold code facts; fresh agents hold no assumptions.
2. **Stop only when the answer changes the plan** and the user knows something the planner cannot look up. Zero stops is the normal case.
3. **Mechanise facts and prefer sound checks.** A check that ran outranks a model that agreed.
4. **Freeze intent, not tasks.** Plans change during execution at 0.89 deviations per task, so change is audited rather than forbidden.
5. **Derive state from artefacts.** No stored stage, no state machine, and preconditions only at the verbs that need them.
6. **Every step has an off-switch and a stated assumption**, so it can be measured and removed.

### Flow

The conversation owns dialogue and gates, because a Workflow has no user hook and `AskUserQuestion` is unavailable in subagents. Each fan-out is one saved Workflow that takes its work-list in `args` and returns a digest and pointers, and every step writes to disk. A new session resumes the plan from those files but not an interrupted run, which it has no way to relaunch, so the carrier computes what is still pending before launch (see Risks). `tomlctl plan stage` says where a plan stands on re-entry.

| Step | Who | Output | Skipped when |
|---|---|---|---|
| Open | Orchestrator, tool | `docs/flows/<slug>/brief.md` distilled from the discussion and any named backlog items | Never |
| Investigate | Workflow | `docs/flows/<slug>/notes/*.md`, each opening with a one-line verdict and `file:line` anchors; index returned | No unknowns, or backlog evidence answers them |
| Structure challenge | One fresh agent | A target structure drafted without seeing the planner's thinking | Reason recorded, such as a one-file fix |
| Surface | Orchestrator, with the user | Zero to three vetoable assumptions | Nothing passes the tests, which is normal |
| Design | Orchestrator | Decisions, contracts, milestones, `docs/flows/<slug>/design/*.md` only where prose is needed | Never for decisions; `design/` when unneeded |
| Surface again | Orchestrator, with the user | Anything design itself raised | As above |
| Write | Orchestrator, one continuous pass | Task specs | Never |
| Prove | Tools in the carrier; a Workflow only for the cold readers | Cold-read results, probes, store checks | Cold read skipped for a plan of a few deep tasks |
| Review | Workflow (Slice 3), then the orchestrator | Digest and proposed edits, which the orchestrator applies by tier after the run | Low risk, by planner judgement, disclosed at approval |
| Approve | User | Frozen intent and a commit stamp | Never |

Depth is not classified upfront. A plan starts minimal and grows on observed results: located files, a failed probe, a failed premise, a one-way door. The research found self-classification the weakest scaling signal, and each row above carries its own skip instead. `--depth` overrides, and investigators are capped at 6.

The orchestrator reads source itself for any fact a decision rests on, because summaries answered 4 of 45 behavioural questions against 27 of 45 for source.

### Surfacing

This replaces fixed frame and align grills. Dependency choices, scoping and large restructuring are attention prompts, not gates, because no evidence favours named categories over judgement, and an agent's own decisions to ask are a weak signal either way (Opus 4.6 on HiL-Bench's software tasks: 34.6% recall, 26.3% precision against registered blockers). That figure warns against this design as much as for it, so the three tests below carry the weight and `vetoed` measures the result. A candidate reaches the user only if it passes all three tests:

1. What would change if the answer were no? It must be a different plan.
2. Can I settle it by reading or running something? Then settle it.
3. Does a recorded user stance already answer it? CLAUDE.md, memory and ADRs count.

Each surfaced item shows the claim, the evidence read, what changes if it is false, and the default. It comes before elaboration, so a wrong assumption is vetoed while that is still cheap. Everything else becomes a decision with provenance `agent-assumed`, disclosed at approval. Unattended, the planner proceeds on defaults and posts a durable question, the one part of Lumina's autonomous mode worth keeping.

For dependencies, tools settle existence, version, licence, advisories and transitive cost: registry with a standard-library allowlist, OSV, `cargo tree` delta. The user sees need, options including no dependency, live facts, removal cost, and a note that a model's preference can reflect training recency.

For scope, a mechanical call-site sweep (`tomlctl sweep`) catches under-scoping. Only readings that lead to materially different work are surfaced.

### Structure challenge

The measured problem is narrower than a status-quo disposition: told only a focus area, agents rarely land the refactoring a human chose, but match it given the detail (GPT-5.2 7.9% against 69.6% on CodeTaste, an alignment score over refactoring tasks). That agents miss a restructuring need unprompted is inferred from this, not measured by it. Other results run the opposite way, with newer models building more custom code and generic LLM decisions favouring change. So a "be bolder" prompt would misfire, and a restructure must clear the same evidential bar as an extension.

One fresh-context agent receives only the intent, the expected future state and the code, and drafts the structure this work would want. Same-thread reflection is excluded because it does not debias. The planner then chooses or vetoes and never merges, since merged variants scored lower. Computed signals such as churn and file count are input to that agent; a score alone never reaches the user, because 63.1% of one detector's architecture smells were false. What can surface is a claim of the form "this module will force X on this change", backed by code the agent read.

Evidence here is adjacent and small, so Slice 2 opens with a replay over 5 to 10 past plan goals, counting divergences the user rates as worth seeing. If that yields little, the step comes out.

### Flow directory

```
docs/flows/<slug>/
  brief.md      Intent, Non-goals, Scope budget, Success criteria, Background, Unknowns
  design/*.md   optional, cited by tasks as design/x.md#anchor
  notes/*.md    evidence; a newer note replaces an older one, with no chain
.claude/flows/<slug>/
  plan.toml     decisions, contracts, approvals, runs, handover (verb-owned)
  tasks.toml    the existing task store plus additive side tables (verb-owned)
```

A flow spans two directories because writes under `.claude/` are protected paths: they prompt in `default` and `acceptEdits`, are denied in `dontAsk`, go to the classifier in `auto`, and `permissions.allow` cannot pre-approve them. tomlctl's stores escape this through `Bash(tomlctl *)`, but files agents author with Write and Edit do not, so those live in `docs/flows/<slug>/`, a fixed repo-relative path since `plansDirectory` may be unset or an array. Unattended `dontAsk` runs still need an `Edit(docs/flows/**)` allow rule, which Slice 2's carrier must document.

`brief.md`, `plan.toml`, `tasks.toml` and `design/` are git-tracked, as `tasks.toml` already is, so `git diff <approved_commit>..HEAD` gives baseline comparison for free. Whether `notes/*.md` is tracked is open: it holds investigator output, this repo is public, and `.gitignore` keeps free text and backlog evidence out on that ground. Slice 2's carrier settles it before it commits a flow directory; nothing in this slice reads notes from git. `context.toml` `plan_path` points at `docs/flows/<slug>/brief.md`, which keeps flow resolution and doctor working unchanged. `.claude/flows/**` is pinned to LF in `.gitattributes`, because `core.autocrlf` otherwise rewrites stores on checkout and fails their sidecars.

`tasks.toml` keeps its verbs: `ready`, `train`, `closure`, `check`, `snapshot` and glimpse all carry over. `import-plan` and `render` refuse a flow that has a `plan.toml`, since a v2 flow has no markdown to import. There is one read-only human view, printed by `plan view`, never written to disk, so nothing can edit it by mistake.

### Task spec

Additive side tables in `tasks.toml`, omitted while empty, so a v1 store stays byte-identical. A store holding any of them stamps `schema_version = 2`, so an older binary refuses it rather than silently dropping them. A side table is chosen over row fields because `TaskRow` struct literals sit in 14 files, and three side tables already key on `ref`. `items list --array specs` keeps the query surface.

```toml
[[specs]]
ref = "add-the-retry-guard"
outcome = "what must be true afterwards"
constraints = ["must not change the public signature"]
accept = [{ behaviour = "a retry after 429 waits the header's delay", check = "cargo nextest run -E 'test(retry_after)'", kind = "forward", positive = "", writes_build = true, probe = { at = "<sha>", neg_rc = 1, pos_rc = 0, verdict = "healthy" } }]
premises = [{ claim = "fn parse exists in src/a.rs", check = "grep -q 'fn parse' src/a.rs", at = "base" }]
means = [{ text = "use the backoff helper", binding = "required", detail = "why this is required" }]
design = ["design/store.md#locking"]
precedent = ["src/b.rs:40"]
executor = "lite"

[[milestones]]
id = "A"
gates = ["cargo nextest run --manifest-path x/Cargo.toml"]
elaboration = "upfront"   # or "deferred"
defer_reason = ""

[[changes]]
at = 2026-11-05
ref = "add-the-retry-guard"
field = "spec.outcome"
old = "..."
new = "..."
actor = "orchestrator"
reason = "..."
```

- `behaviour` is the orchestrator's and states what must be observably true. A cold reader may turn it into `check`, but may not change what it asserts, because LLM oracles capture actual over intended behaviour.
- `check` must exit non-zero when the behaviour does not hold, the same contract `success:` keys already have. This is what makes acceptance machine-runnable for the first time. `positive` is an optional command for the positive control, empty when there is none. `writes_build` absent counts as true, so only a row explicitly marked `false` is probed. `probe` is written only by `plan probes --record`; `add-many` refuses a supplied one.
- `means` is empty by default. A `required` entry needs a reason in its own `detail`, because wrong or over-specified instructions outnumber missing ones in this repo's own data. Clerks need enumerated targets, since Sonnet 5 does not generalise an instruction from one item to the next. Deep tasks get a scope clause, since Opus 5 can expand scope.
- A false premise goes to replanning, never up a tier: a stronger executor does not repair a wrong plan. `at` is `base` or `after:<id>`.
- `files` stays `Vec<String>` with `[[file_notes]]` for `(new)` and `(delete)`. The idea doc's `{path, change}` shape would break every file-claim consumer and glimpse's model for no gain. On a v2 flow, `tasks add-many` rows carry those annotations in a `file_notes` key and backlog links in `closes` and `refs` keys, because `import-plan` is the only other writer of either table. `[policy]` stays at its defaults, since no verb writes it.
- A deferred milestone's tasks carry outcome, behaviour, constraints and estimated files; `check`, premises and precedent arrive when it is elaborated.

### Plan records

`plan.toml`, written only through `plan` verbs:

```toml
[binding]
branch = "main"                # held here until approval, so a draft matches no resolver step
scope = ["src/**"]

[[decision]]
id = "D1"
decision = "what to do, phrased as an instruction"
why = "one line"
provenance = "agent-assumed"   # user-stated | user-confirmed | agent-assumed
door = "two-way"               # one-way | two-way
binds = ["3", "src/a.rs"]
rejected = [{ option = "...", because = "the fact that sank it" }]
evidence = "what was read"
if_false = "what changes"
revisit_if = ""
check = ""
surfaced = false
vetoed = false

[[contract]]
id = "C1"
kind = "invariant"             # invariant | interface
claim = "..."
check = "..."
tasks = [3, 5]
provides = ""
consumes = ""

[[approval]]
commit = "<sha>"
at = 2026-11-05T12:00:00Z
brief_sha256 = "..."
by = "user"
review = "ran"                 # ran | skipped-low-risk | skipped-flag
reason = ""

[[run]]
step = "investigate"
workflow = "plan-v2-investigate"
args_hash = "..."
run_id = "wf_..."
started = 2026-11-05T12:00:00Z
finished = ""
skip_reason = ""               # set by `plan run skip` for a step not run

[[handover]]
id = "H1"
command = "cargo install --path tomlctl"
source_task = 3
done = false
```

A surfaced item is simply a decision with `surfaced = true`, so there is one record type, and `vetoed` gives a measure of whether surfacing earns its place. Decisions are injected into the tasks they bind, because storage location decides whether a record gets read. `[[contract]]` exists because the record otherwise has no cross-task object, the gap behind Lumina's invariant that fell between two tasks; it is checked at the join, not inside either task. The remedy's evidence is low grade, so outcomes should be watched.

An entry in `binds` that is all digits names a task id, which must be a row; any other entry is a path, which must be on some row's `files` or exist under the repo root.

No supersession chains, no option-to-task branches, no rework epochs: Lumina measured those at 7 of 353, 0 of 41 and 0 of 696. No field catalogue copy either, until a reader needs a field hidden.

### Verbs

All take `--slug` and follow `tomlctl/src/review/dispatch.rs`'s shape, with store access through `tomlctl/src/tasks/store.rs`'s `mutate` pattern.

| Verb | Does |
|---|---|
| `plan open --slug <s> [--from <id>[,<id>...]] [--branch <b>] [--scope <glob>]...` | Writes the `brief.md` skeleton, calls `flow init` with no branch or scope, holds the given ones in `plan.toml`'s `[binding]`, removes the registry entry so a draft is never auto-picked. Named backlog rows map summary to Intent, context to Background, evidence to a pointer note. Idempotent on its own flow; refuses a flow whose `plan_path` is anything else. Prints `{ok, slug, brief, created}` |
| `plan decide`, `plan contract` `--ndjson <path>` | Add or update records, ids minted under the lock. A `.toml` record file works, as on every payload flag. Prints `{ok, ids}` |
| `plan handover add --command <c> --source-task <n>`, `plan handover done <id>`, `plan run start --step <s> --workflow <w> --args-hash <h> [--run-id <id>]`, `plan run finish --step <s> [--run-id <id>]`, `plan run skip --step <s> --reason <text>` | Their records. `run start` is written immediately on launch, `run finish` closes the newest unfinished `[[run]]` for its step, and `run skip` records a step not run. Prints `{ok, id}` for handover and `{ok}` for run |
| `plan milestone add --id <A> --goal <text> [--gate <cmd>]... [--defer --reason <text>]`, `plan milestone update --id <A> [--goal <text>] [--gate <cmd>]... [--defer] [--reason <text>]` | Writes `[[checkpoints]]` and `[[milestones]]` in one lock, the first verb besides `import-plan` that can. `--defer` needs `--reason`, and so does any `update` on an audited store. Prints `{ok, id}` |
| `plan check` | Runs `tasks check` plus the classes below. Prints `{ok, findings[]}`, the `tasks check` shape. Exits 1 on any error |
| `plan view --for human\|digest\|worker [--task <id>]` | The three projections. Worker inlines Intent, Non-goals, bound decisions, contracts, the spec and the cited design sections, because agents rarely pull |
| `plan probes --emit` / `--record <path>` | See Proving |
| `plan stage` | Computes `{stage, next, blockers}` from what exists. Never stored |
| `plan gate` | The approval screen's data, one object with an array per item under Approval |
| `plan approve --review ran\|skipped-low-risk\|skipped-flag [--reason <text>] [--amend]` | Preconditions, then stamps. `ran` needs a finished plan-family run in `review-v2.toml`, and both skips need `--reason`. Prints `{ok, commit, transition: {from, to}}`. See Approval |
| `plan drift` | Files changed since approval, minus this plan's commits, intersected with task files. Prints one row per affected task. Warns and exits 0; only a plan with no approval is refused |

Check classes: `plan/brief-missing`, `plan/brief-section-empty` (Intent, Non-goals, Scope budget, Success criteria), `plan/brief-changed` (hash differs after approval), `plan/design-anchor-dead`, `plan/contract-no-check`, `plan/contract-task-unknown`, `plan/decision-bind-unknown`, `plan/task-no-spec`, `plan/spec-no-outcome`, `plan/spec-no-accept`, `plan/accept-no-check` (upfront milestones only), `plan/means-required-no-reason`, `plan/deferred-no-reason`, `plan/milestone-no-gate` (warning), `plan/probe-unhealthy` (any recorded verdict but `healthy`), `plan/probe-absent` (warning, for an `accept` with a non-empty `check`, `writes_build = false` and no probe), `plan/spec-orphan` (a spec keyed on a `ref` no row carries).

### Proving

**Probes.** `plan probes --emit` prints a Bash script: the existing helper from `claude/skills/flow-contract-plan-output-format/references/acceptance-probing.md`, one line per check, skipping every row not marked `writes_build = false`. The carrier runs that script through the Bash tool, and `--record` stores the results and computes verdicts from the existing table: `healthy`, `vacuous`, `unsatisfiable`, `permanently-green`, `broken` or `guard-alone`. tomlctl deliberately does not execute the commands itself: `Bash(tomlctl *)` is pre-approved, so a verb running stored strings would widen that approval to arbitrary commands. This is not a control on what the script runs: Bash rules match command text, so `bash <file>` shows no check, and under `bypassPermissions` nothing prompts. A stored `check` is repo code run with the user's rights, on the same terms as a `success:` key.

A check that already passes at base is vacuous or its premise is false, which is the power test the research supports. Agent test patches carry weak oracle signals 80.2% of the time, and a label records polarity, not power. Reverting hunks after completion to test power is a hypothesis for the engine's slice.

**Cold readers.** One fresh read-only agent per upfront milestone gets only the worker projection and the code, and reports what it cannot act on, false premises and exact check commands. The orchestrator applies results through verbs and resolves anything touching intent. This replaces the idea doc's per-milestone writers. Agents never write the task store, so task-store section 11's one-writer rule stands.

### Approval

`plan gate` feeds one screen, in this order, with counts last, because counts reassure without giving a reason to think:

1. Agent-assumed decisions, ranked by how much they bind, each with `revisit_if`.
2. One-way doors the plan will pass.
3. Top risks from review when it ran, or the recorded reason it was skipped.
4. The plan's task mix by file-count bucket (1, 2 to 3, 4 or more) and executor.
5. Review edits that altered a decision (Slice 3).
6. Deferred milestones and why.
7. What will stop the run: milestone gates and undone handovers.

Any answer beyond "approve" routes into the plan. `plan approve` requires `plan check` clean and every surfaced decision answered, then stamps `[[approval]]` with `HEAD` and the brief's hash, taken over LF-normalised bytes so that line endings alone never read as a change to intent, sets `audit = true` on the task store, moves `status` from `draft` to `in-progress`, writes the held `[binding]` branch and scope to `context.toml`, adds the registry entry, and promotes `closes` backlog links. The carrier commits the flow directory first, so the stamp names a commit that holds the plan. `--amend` appends a new approval after a user-agreed change to intent, and the old stamp stays.

From then on `tasks add-many`, `update`, `remove` and `plan milestone update` need a `reason` for any plan field, and each writes a `[[changes]]` row, with tombstones for removals; a milestone's rows use `ref = "milestone:<id>"`. Single-row `tasks add` and the positional `update` forms have no reason channel, so they are refused on these fields. `status`, `agent` and `commit` are unaudited. `draft` therefore means unapproved: drafts have no registry entry, scope or branch, because flow resolution reads every non-complete flow on disk, so automatic resolution does not pick them, and `flow list --status draft` still lists them, with the `updated` a chooser computes age from. That resolves the idea doc's conflict between resumable plans and hidden drafts.

### After approval

Specified here for the engine's own plan to build; none of it is built in this plan.

- **Deferred elaboration.** Upfront is the norm. The planner may defer a later milestone, with a stated reason, when its detail depends on what earlier milestones will produce; its high-level shape is reviewed upfront. Just before that milestone implements, the engine runs elaboration and review as two top-level Workflow runs with the orchestrator between them, because review reads the elaborated specs, agents never write the task store, the orchestrator cannot act during a run, and `workflow()` nests one level only. The Elaborate run writes its proposals to a file and returns a pointer; the orchestrator applies them with `tasks update --ndjson` and a reason, then starts the Review run. A resulting change to an outcome or success criterion goes to the user.
- **Replan at each milestone drain.** The orchestrator owns it. Inputs are deviations, failed premises, worker handoffs and `plan drift`. Output is task changes through audited verbs. A digest at the end cannot help tasks that already ran on a falsified assumption.
- **Tripwires.** Interrupt only for a one-way door no decision covers, a contradiction of a decision or outcome, a failed premise that replanning cannot settle, or a budget breach. Park that branch and continue disjoint work. Everything else goes in the milestone digest.
- **Task contract freezes at dispatch.** Mid-task change is where Devin reports weakness.

### What departs from the idea doc

| Idea doc | This design | Why |
|---|---|---|
| Tasks: orchestrator skeleton, fresh writers fill in | Orchestrator writes intent; cold readers challenge | User decision 2; agent-written guidance scored below none |
| Approved plan frozen, changes as events | Intent frozen, tasks audited, no fold | User decision 3; 433 prose deviations cannot fold |
| Deviations digested at the end | Replan at each milestone drain | No replanning owner existed |
| Question stage before design | Surfacing by judgement, before elaboration | User decision 4 |
| Auto-apply verified edits | Tier by evidence | User decision 6; models over-reject |
| Review opt-out question | Flags, low risk disclosed at approval | User decision 5; the question fired before the design it assessed |
| Gate shows counts | Assumptions first, counts last | Counts invite a rubber stamp |
| `files = [{path, change}]` | `Vec<String>` with `[[file_notes]]` | Avoids breaking every consumer |
| `/plan` | `/plan-v2` while v1 coexists | `/plan` is a built-in |
| "Scales by itself" | Starts minimal, grows on observed results | Self-classification is the weakest signal |
| Draft hidden from resolution | Unapproved means excluded from auto-pick, listed in the chooser | Must stay resumable |
| Clerk's one-line Haiku swap, no pilot | Out of scope here; flagged for the engine plan | Haiku 5.5's guide says to compare effort levels on your own evals |

Kept: flow directory, projections, render never feeds back, milestones as the only checkpoint shape, `[[handover]]`, Workflows for fan-out, blind finders with verdict before notes, the bounded re-check.

The older practitioner guide at `docs/planning research.md` agrees on "tight on WHAT, loose on HOW" and an explicit "not doing" section. It is a secondary source and its figures were not re-checked.

### Later slices

Each becomes its own `/plan-new` once the slice before it lands, because each one's detail depends on verbs and record shapes this slice settles.

| Slice | Delivers | Files, estimated | Enters when |
|---|---|---|---|
| 2 | `claude/commands/plan-v2.md`, `claude/skills/plan-v2-engine/`, agents `plan-challenger` and `plan-cold-reader`, Workflows `plan-v2-investigate` and `plan-v2-prove`, the challenger replay, the three Workflow resume tests under Risks, `VALID_COMMANDS` and `carrier_invokes_required_skills`; settling whether `notes/` is tracked, running the probe script in the carrier (Bash to a scratch file, then `--record`, since a Workflow script cannot run tools), and appending the `status-transition` record `plan approve` reports | ~12 | Slice 1 merged and installed, and a working deploy check passes: saved Workflows resolve by name from `workflows/` under the active Claude config dir (`CLAUDE_CONFIG_DIR`, default `~/.claude`). `deploy-claude.ps1` does not read that variable today, so where it is set, a default `-Check` reports on a directory the session does not read |
| 3 | `/review-plan-v2` bound to a flow directory, findings targeting records, tiered edit application, the review wrapper Workflow, plan-review outcome logging | ~15 | Slice 2 can open a real plan |
| 4 | glimpse shows `brief.md`, decisions, stage and approval state | ~6 | Slice 3 merged |

The execution engine remains topic 4 of the idea doc. It must honour After approval above and read specs through `plan view --for worker`.

## Success Criteria

- forward: the plan lifecycle test passes: a draft opens, stays out of automatic resolution, takes decisions, milestones and specs, proves, approves, promotes its backlog link, then audits a change and reports drift. `cargo test --manifest-path tomlctl/Cargo.toml --test plan_lifecycle` **predicted, unverified**, since it writes build output and the test does not exist yet.
- forward: `grep -q 'Plan {' tomlctl/src/cli/types/cmd.rs` exits 0 (today: exits 1).
- forward: `test -f claude/skills/tomlctl/references/plan.md` exits 0 (today: exits 1).
- forward: plan-family cleanup rows need no `old`, which is what closes `B-ef306556`. `cargo test --manifest-path tomlctl/Cargo.toml --test review_add plan_cleanup_row_needs_no_old` passes **predicted, unverified**; falsifier: removing the family check in `old_required` turns it red.
- forward: `test -f docs/ideas/v2-pipeline.md && ! grep -q 'Planning flow: draft' docs/ideas/v2-pipeline.md` exits 0 (today: exits 1).
- forward: no tracked flow file is CRLF on disk. `test -z "$(git ls-files --eol .claude/flows | grep 'w/crlf')"` exits 0 (today: exits 1).
- guard: v1 task-store behaviour is unchanged. `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_render --test tasks_import --test tasks_read --test tasks_write` passes, and so does `cargo test --manifest-path tomlctl/Cargo.toml --lib tasks::schema`, which holds the test that a store with no specs round-trips byte-identical **predicted, unverified**.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
test.rerun: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -j 1 -- --exact {ids}
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
checkpoint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
checkpoint: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
checkpoint: cargo clippy --manifest-path glimpse/Cargo.toml --locked
success: grep -q 'Plan {' tomlctl/src/cli/types/cmd.rs
success: test -f claude/skills/tomlctl/references/plan.md
success: test -f docs/ideas/v2-pipeline.md && ! grep -q 'Planning flow: draft' docs/ideas/v2-pipeline.md
success: test -z "$(git ls-files --eol .claude/flows | grep 'w/crlf')"
test.timeout: 900
checkpoint.timeout: 900
transient: rust-lld: failed to write output.*[Pp]ermission denied
```

The 900-second budgets are from 2026-11-05: the suite took 322 seconds on a quiet machine, and under four concurrent research agents reached 1283 of 2302 tests in 300 seconds. Each measured `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast`.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 3, 5, 8, 10, 11, 15, 20, 21, 22, 24, 25, 26, 27, 28, 29, 30, 31
- **Max parallel agents**: 8
- **Commit granularity**: per-task

## Tasks

### Phase 0: Repair the baseline

#### 27. Fix the id_aliases twin timing flake [S]
- **Files**: `tomlctl/tests/id_aliases.rs`
- **Depends on**: —
- **Action**: Make `assert_same` compare two twins built from one seeding, so they cannot differ in their timestamps.
- **Detail**: `twin()` runs `backlog add` twice and backlog rows carry one-second `Z` datetimes, so two separate seedings differ whenever seeding crosses a second boundary. Measured 2026-11-05: 6 of the binary's 7 tests failed in a full nextest run and 2 still failed at `-j 1`, so `test.rerun` does not absorb it. Seed one sandbox, copy its whole tree into a second temporary directory, and leave the rest of `assert_same` as it is. Test-only, and it comes first because every checkpoint gate runs this suite.
- **Acceptance**: forward: `cargo test --manifest-path tomlctl/Cargo.toml --test id_aliases` passes **predicted, unverified** (today: fails intermittently); falsifier: seeding the twins separately again fails whenever seeding crosses a second.

### Phase 1: Task store additions

#### 1. Add the spec side tables to the task store [M]
- **Files**: `tomlctl/src/tasks/schema.rs`
- **Depends on**: —
- **Action**: Add `TaskSpec`, `Accept`, `Probe`, `Premise`, `Means`, `MilestoneSpec` and `Change` types, and `specs`, `milestones`, `changes` and `audit` fields on `Store`, with `from_toml` and `to_toml` conversions.
- **Detail**: Shapes are in Approach, "Task spec". Follow `FileNote` and `BacklogLink`: keyed by `ref` (`MilestoneSpec` by checkpoint id), omitted on write while empty, `audit` omitted while false, an entry with no key dropped on read. Keep unknown vocabulary as strings here; validation belongs to the writers, as with `Policy`. Add `..Store::default()`-compatible defaults so existing struct literals still compile. Raise `SCHEMA_VERSION` to 2 and have `to_toml` stamp 2 only when `specs`, `milestones`, `changes` or `audit` is present, otherwise 1, with `Store::default()` staying at 1, because the installed binary refuses a newer version but drops these keys silently at version 1. Add `specs` to `drop_ref_entries`, and never `changes`, since a tombstone must outlive the row it records. Inline tests: a store with none of these round-trips byte-identical at version 1, and one with all of them round-trips exactly at version 2.
- **Acceptance**:
  - forward: `grep -c 'pub(crate) struct TaskSpec' tomlctl/src/tasks/schema.rs` prints `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib tasks::schema` passes **predicted, unverified**; falsifier: emitting empty side tables turns the byte-identical test red.
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_render` passes **predicted, unverified**; falsifier: emitting empty side tables would make it red.

#### 2. Let add-many carry a task spec [M]
- **Files**: `tomlctl/src/tasks/add.rs`, `tomlctl/tests/tasks_spec_add.rs` (new)
- **Depends on**: 1
- **Action**: Accept a `spec` key on `tasks add-many` rows, validate it, and store it in `[[specs]]` keyed by the minted `ref`, with `file_notes`, `closes` and `refs` keys stored in their side tables the same way.
- **Detail**: Add `spec`, `reason`, `actor`, `file_notes`, `closes` and `refs` to `ROW_FIELDS`, and leave the other accepted keys alone. Keep them beside `NewTask` rather than on it, so `tomlctl/src/tasks/dispatch.rs` stays untouched. `file_notes` maps a path on the row's `files` to its annotation, and `closes` and `refs` list backlog ids; they write `[[file_notes]]` and `[[backlog_links]]` because `import-plan` is the only other writer of either table and task 6 refuses it on a v2 flow. Refuse, with the row's index: `kind` outside forward, falsifier, guard; `binding` outside required, suggested; `executor` outside tool, clerk, lite, deep; `at` that is neither `base` nor `after:<id>` naming a row in the store or batch; a supplied `probe`; unknown keys inside `spec`. When `store.audit` is true, require a per-row `reason` and write a `[[changes]]` row with `field = "*"`, an empty `old` and the title as `new`, since a task added after approval is a plan change. `tasks add` carries no spec or reason, so on an audited store it is refused with a message naming `add-many`. Test that a `.toml` record file works through `--ndjson`, because that is how the orchestrator will author tasks with prose bodies.
- **Acceptance**:
  - forward: `grep -c '"spec"' tomlctl/src/tasks/add.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_spec_add` passes **predicted, unverified**; falsifier: removing the `kind` check leaves a bad-vocabulary case accepted, turning it red.

#### 3. Let update reach specs and audit changes [M]
- **Files**: `tomlctl/src/tasks/update.rs`, `tomlctl/tests/tasks_audit.rs` (new)
- **Depends on**: 1, 2, 6
- **Action**: Let `tasks update --ndjson` rows carry `spec`, `reason` and `actor`, re-key a row's spec on `--ref`, and write `[[changes]]` under an audited store.
- **Detail**: Rows gain optional `spec` (each given sub-field replaces the stored one, validated as in task 2), `reason`, `actor` (default `orchestrator`). Under `audit = true`, a change to `title`, `effort`, `action`, `detail`, `acceptance`, `files`, `needs`, `coupling`, `deps_note`, `checkpoint`, `ref` or `spec.*` without a `reason` refuses the whole batch and names the row; each moved field writes one `Change`. A `ref` change also rewrites earlier `[[changes]]` rows to the new `ref`, so a task's trail stays whole. `status`, `agent`, `commit` stay unaudited. A spec change through the positional form, and any audited change there, is refused with a message naming `--ndjson`, since it has no reason channel. Files and needs on a v2 flow (`is_v2_flow`, task 6), audited or not, need no `--unlock-import-fields` and write no `[[import_overrides]]` stamp, since it has no import. `batch_row` calls `settable` at parse time, so that check moves to where the store path is known. Dates come from `crate::time`.
- **Acceptance**:
  - forward: `grep -c 'changes' tomlctl/src/tasks/update.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_audit` passes **predicted, unverified**; falsifier: skipping the `reason` check turns the refused-without-reason case red.

#### 4. Tombstone removals on an audited store [M]
- **Files**: `tomlctl/src/tasks/remove.rs`, `tomlctl/src/tasks/dispatch.rs`, `tomlctl/src/cli/types/tasks.rs`
- **Depends on**: 1
- **Action**: Add `--reason` to `tasks remove`, require it under an audited store, write a tombstone `Change`, and prune the row's spec.
- **Detail**: A tombstone is `field = "*"`, `new = ""`, with `old` holding the removed title, and a `--force` removal also writes one `needs` `Change` per row it rewires. Prune `[[specs]]` on any successful removal; `[[file_notes]]` and `[[backlog_links]]` stay as they are today. Without `audit`, behaviour is unchanged apart from the prune. Add inline tests in `tomlctl/src/tasks/remove.rs`.
- **Acceptance**:
  - forward: `grep -c 'reason' tomlctl/src/tasks/remove.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib tasks::remove` passes **predicted, unverified**; falsifier: skipping the tombstone write turns the audited-removal case red.
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_write` passes **predicted, unverified**.

#### 5. Show a task's spec [S]
- **Files**: `tomlctl/src/tasks/show.rs`, `tomlctl/src/cli/types/tasks.rs`, `claude/skills/tomlctl/references/tasks.md`
- **Depends on**: 1, 4
- **Action**: Add a `spec` part to `tasks show --with`, reporting the row's `[[specs]]` entry.
- **Detail**: The `--with` selector enum lives in `tomlctl/src/cli/types/tasks.rs`. Follow how the `files` part reports `file_notes`. Absent spec prints no key, so existing output is unchanged. Add `spec` to the valid `--with` set in `claude/skills/tomlctl/references/tasks.md`, which enumerates it. Inline test both shapes.
- **Acceptance**:
  - forward: `grep -c '"spec"' tomlctl/src/tasks/show.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib tasks::show` passes **predicted, unverified**; falsifier: omitting the absent-spec case leaves it unasserted, and printing a key for it turns that case red.

#### 6. Refuse markdown import and render on a v2 flow [S]
- **Files**: `tomlctl/src/tasks/store.rs`, `tomlctl/src/tasks/dispatch.rs`, `tomlctl/tests/plan_refusals.rs` (new)
- **Depends on**: 4
- **Action**: Refuse `tasks import-plan` and `tasks render` when the store's directory holds a `plan.toml`, and skip `tasks check --plan`'s render-drift comparison there.
- **Detail**: Add `is_v2_flow(store_path)` to `tomlctl/src/tasks/store.rs` and call it at the two arms in `tomlctl/src/tasks/dispatch.rs`, with `kind=validation` and a message naming `plan view`. Also call it at the `--plan` branch of `tasks check`, because `brief.md` holds no rendered sections and the drift it would report names a remedy this task refuses. Plan-mode `import-plan --plan --dry-run` has no store and is untouched. Why: an import would overwrite plan-owned row fields from a document that does not exist.
- **Acceptance**:
  - forward: `grep -c 'is_v2_flow' tomlctl/src/tasks/dispatch.rs` prints at least `2` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_refusals` passes **predicted, unverified**.

### Phase 2: Plan store and command group

#### 7. Add the plan store [L]
- **Files**: `tomlctl/src/plan/mod.rs` (new), `tomlctl/src/plan/schema.rs` (new), `tomlctl/src/plan/store.rs` (new), `tomlctl/src/lib.rs`
- **Depends on**: —
- **Action**: Create the `plan` module with typed `Binding`, `Decision`, `Contract`, `Approval`, `Run` and `Handover` records, and locked load and mutate over `.claude/flows/<slug>/plan.toml`.
- **Detail**: Shapes are in Approach, "Plan records". Model `tomlctl/src/tasks/store.rs` and `tomlctl/src/tasks/schema.rs`: `resolve` validates the slug through `crate::flow::validate_slug`, `mutate` restamps `last_updated`. Unknown keys are dropped, as in the task store, so copy its refusal of a newer `schema_version`. Add `brief_digest(bytes) -> String` to `tomlctl/src/plan/store.rs`: SHA-256 over the bytes with CRLF folded to LF, inline-tested so that LF and CRLF copies of one text digest alike and a changed word does not. Tasks 14 and 18 both call it, so one rule decides what counts as a changed brief. These edits are inseparable: the module must be declared in `tomlctl/src/lib.rs` to compile. Task 9's stubs consume nothing and the verb tasks land in parallel, so put `#[allow(dead_code)]` on the `plan` module declaration, and task 20, the join of every verb, removes it.
- **Acceptance**:
  - forward: `grep -c '^mod plan;' tomlctl/src/lib.rs` prints `1` (today: `0`).
  - forward: inline round-trip tests pass under `cargo test --manifest-path tomlctl/Cargo.toml --lib plan::` **predicted, unverified**.

#### 8. Guard and seed plan.toml [S]
- **Files**: `tomlctl/src/store_guard.rs`, `tomlctl/src/io.rs`, `tomlctl/src/cli/types/shared.rs`
- **Depends on**: —
- **Action**: Add `.claude/flows/<slug>/plan.toml` to the owned stores and to the schema-seeded flow files.
- **Detail**: Add `OwnedStore::Plan` with a replacement message naming the `plan` verbs, extend the `store_guard_refuses_each_owned_store` list, and add `plan.toml` to the seeded basename list beside `tasks.toml` in `tomlctl/src/io.rs`. Reword `OwnedStore::Tasks`'s message, which says no verb but `tasks import-plan` writes `[[checkpoints]]`: `plan milestone` will too. Keep the words `tasks import-plan`, which `store_guard_message_names_the_replacement_and_bypass` asserts. Add `plan` to the store list in the `--raw-store` help in `tomlctl/src/cli/types/shared.rs`.
- **Acceptance**:
  - forward: `grep -c 'OwnedStore::Plan\|Self::Plan' tomlctl/src/store_guard.rs` prints at least `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib store_guard` passes **predicted, unverified**.

#### 9. Scaffold the plan command group [L]
- **Files**: `tomlctl/src/cli/types/plan.rs` (new), `tomlctl/src/cli/types/mod.rs`, `tomlctl/src/cli/types/cmd.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/cli/mod.rs`, `tomlctl/src/tasks/mod.rs`, `tomlctl/src/tasks/backlog_refs.rs`, `tomlctl/src/flow/mod.rs`, `tomlctl/src/plan/mod.rs`, `tomlctl/src/plan/dispatch.rs` (new), `tomlctl/src/plan/open.rs` (new), `tomlctl/src/plan/records.rs` (new), `tomlctl/src/plan/milestone.rs` (new), `tomlctl/src/plan/check.rs` (new), `tomlctl/src/plan/view.rs` (new), `tomlctl/src/plan/probes.rs` (new), `tomlctl/src/plan/stage.rs` (new), `tomlctl/src/plan/approve.rs` (new), `tomlctl/src/plan/drift.rs` (new)
- **Depends on**: 7
- **Action**: Add the `tomlctl plan` subcommand with every verb's clap arguments, and a dispatch to a stub in each verb module.
- **Detail**: Verbs and flags are in Approach, "Verbs". Model `tomlctl/src/cli/types/review.rs` and `tomlctl/src/review/dispatch.rs`, and include `--error-format` behaviour through the existing global flags. Each stub returns a `kind=validation` "not implemented" error. Every later verb task then owns one module, which is what makes them parallel. Add `"plan"` to `SUBCOMMANDS`, and re-export `PlanOp` and its argument types from `tomlctl/src/cli/mod.rs`, whose list is explicit. `crate::plan` is a sibling of `tasks` and `flow`, whose leaves are private, so make `check`, `finding`, `schema`, `slug` and `backlog_refs` `pub(crate) mod` in `tomlctl/src/tasks/mod.rs`, widen `linked_backlog_findings` to `pub(crate)` in `tomlctl/src/tasks/backlog_refs.rs`, and make `active` and `init` `pub(crate) mod` in `tomlctl/src/flow/mod.rs`. A widened `mod` raises no unused-import warning, and the verb tasks then need none of these files. Inseparable: clap, dispatch and stubs must compile together.
- **Acceptance**:
  - forward: `grep -c 'Plan {' tomlctl/src/cli/types/cmd.rs` prints `1` (today: `0`).
  - forward: `grep -c '"plan"' tomlctl/src/cli/types/mod.rs` prints `1` (today: `0`).

#### 10. Advertise plan in capabilities [L]
- **Files**: `tomlctl/src/cli/types/mod.rs`, `tomlctl/README.md`, `tomlctl/src/capabilities.rs`, `tomlctl/tests/capabilities.rs`, `tomlctl/tests/integration.rs`
- **Depends on**: 9
- **Action**: Add a `plan_store` feature and any `MUTEX_GROUPS` the plan verbs declare, and assert the subcommand in the integration list.
- **Detail**: `FEATURES` is in `tomlctl/src/cli/types/mod.rs`, which also says a new subcommand needs one integration assertion. `readme_feature_transcriptions_match_capabilities_features` requires the feature in both the sample block and the Feature meanings table of `tomlctl/README.md`, and `tomlctl/tests/capabilities.rs` lists the expected features. Also update the README's hand-listed subcommands sample and add `plan` to the store list in its `owned_store_guard` row. `MUTEX_GROUPS` in `tomlctl/src/capabilities.rs` covers `--emit` and `--record` on `plan probes`. `--for` and `--task` are not exclusive, since `--for worker` requires `--task`. Inseparable: the README gate goes red on any advertised feature it does not list.
- **Acceptance**:
  - forward: `grep -c '"plan_store"' tomlctl/src/cli/types/mod.rs` prints `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes **predicted, unverified**; falsifier: omitting the feature from `tomlctl/README.md` turns `readme_feature_transcriptions_match_capabilities_features` red.

### Phase 3: Authoring verbs

#### 11. Implement plan open [M]
- **Files**: `tomlctl/src/plan/open.rs`, `tomlctl/src/flow/init.rs`, `tomlctl/tests/plan_open.rs` (new)
- **Depends on**: 8, 9
- **Action**: Implement `plan open`: write the `brief.md` skeleton, bootstrap the flow, seed `plan.toml`, and leave the flow out of the active registry.
- **Detail**: Extract the body of `dispatch` in `tomlctl/src/flow/init.rs` into a callable returning the envelope, with `dispatch` printing it, so behaviour is unchanged. `--from` reads rows through the backlog library and fills Intent and Background; evidence becomes `notes/backlog-<id>.md` listing the pointers and the `.claude/backlog-evidence/<id>/` directory. Re-running keeps an existing `brief.md` and `created`. Refuse a slug whose `context.toml` records any `plan_path` other than this flow's `brief.md`, because seeding `plan.toml` would stop a v1 flow importing or rendering. Hide the draft the way seed flows are hidden: call `flow init` with no `--scope` or `--branch` and remove its registry entry through the helpers in `tomlctl/src/flow/active.rs`, because flow resolution reads every non-complete flow on disk, registered or not. Hold the given `--branch` and `--scope` in `plan.toml`'s `[binding]`; task 18's approve writes them to `context.toml` and the registry entry. Once `plan.toml` holds an `[[approval]]`, leave the registry entry alone. Headings are in Approach, "Flow directory".
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_open` passes **predicted, unverified**; falsifier: leaving the registry entry in place, or writing the scope to `context.toml`, turns the not-auto-picked case red, which asserts `flow resolve --path` on a path matching the scope picks no draft.
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test flow_init` passes **predicted, unverified**.

#### 12. Implement decide, contract, handover and run [M]
- **Files**: `tomlctl/src/plan/records.rs`, `tomlctl/tests/plan_records.rs` (new)
- **Depends on**: 9
- **Action**: Implement the record verbs with id minting under the lock, add-or-update semantics, and vocabulary validation.
- **Detail**: A row with `id` updates; without one, it mints `D<n>`, `C<n>` or `H<n>`, and a removed top id is never reused. Validate `provenance`, `door`, `kind` and the `{option, because}` shape. Refuse unknown keys, as `tasks add-many` does. `binds` is checked by `plan check`, not here, because tasks may not exist yet. `plan run start` returns immediately so it can be written at launch; `plan run finish` closes the newest unfinished `[[run]]` for its step; `plan run skip` appends one with `skip_reason` set.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_records` passes **predicted, unverified**; falsifier: dropping the vocabulary check turns the bad-provenance case red.

#### 13. Implement plan milestone [M]
- **Files**: `tomlctl/src/plan/milestone.rs`, `tomlctl/tests/plan_milestone.rs` (new)
- **Depends on**: 1, 9
- **Action**: Implement `plan milestone add|update`, writing `[[checkpoints]]` (goal as `rationale`) and `[[milestones]]` in one `tasks::mutate_store`.
- **Detail**: `--defer` without `--reason` is refused. On an audited store, `update` requires `--reason` and writes a `[[changes]]` row with `ref = "milestone:<id>"`. `tasks add --checkpoint` then accepts the id through its existing declared-group check. Test that sequence, since that refusal is what blocks direct-to-store plans today.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_milestone` passes **predicted, unverified**; falsifier: writing only `[[milestones]]` turns the checkpoint-accepted case red.

#### 14. Implement plan check [M]
- **Files**: `tomlctl/src/plan/check.rs`, `tomlctl/tests/plan_check.rs` (new)
- **Depends on**: 1, 9
- **Action**: Implement `plan check`: run the task store's checks and add the classes in Approach, "Verbs", exiting 1 on any error.
- **Detail**: Reuse `tomlctl/src/tasks/check.rs`'s engine and finding shape, plus `linked_backlog_findings`, as `tasks check` does. Approach, "Plan records" gives the `binds` rule `plan/decision-bind-unknown` applies, and Approach, "Verbs" the scope of `plan/probe-absent`. Export `design_section(root, "design/x.md#anchor") -> Option<String>` for `plan view`, using the slug rule from `tomlctl/src/tasks/slug.rs` for headings; dead anchors are errors, because a renamed heading is the renamed-heading trap one layer up. `plan/brief-changed` applies only after an approval and compares through `brief_digest`. Accept a seeded fixture directly on disk in tests so this has no dependency on other verbs.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_check` passes **predicted, unverified**; falsifier: a fixture with a dead anchor must report `plan/design-anchor-dead`, so removing that class turns it red.
  - forward: a CRLF copy of an approved brief reports no `plan/brief-changed` while an edited one does, in the same test **predicted, unverified**; falsifier: hashing raw bytes turns the CRLF case red.

#### 15. Implement plan view [M]
- **Files**: `tomlctl/src/plan/view.rs`, `tomlctl/tests/plan_view.rs` (new)
- **Depends on**: 2, 12, 13, 14
- **Action**: Implement `plan view --for human|digest|worker [--task <id>]`.
- **Detail**: Human: the whole plan as one markdown document on stdout, never written. Digest: stage inputs, counts and open items for an orchestrator re-entering, capped by `--max-chars` as usual. Worker: Intent, Non-goals, decisions whose `binds` name the task or its files, contracts naming the task, the row and spec, cited design sections inlined, and each file's kind through `FileKind::of_note` in `tomlctl/src/tasks/schema.rs`, as `show` does. `--for worker` requires `--task`. A fixture test asserts a bound decision's `rejected` text reaches the worker and an unbound decision's does not.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_view` passes **predicted, unverified**; falsifier: inlining every decision turns the unbound case red.

### Phase 4: Proving and approval

#### 16. Implement plan probes [M]
- **Files**: `tomlctl/src/plan/probes.rs`, `tomlctl/tests/plan_probes.rs` (new), `tomlctl/tests/common/mod.rs`
- **Depends on**: 2, 9
- **Action**: Implement `plan probes --emit`, which prints a Bash script, and `--record <path>`, which stores results and verdicts on each `accept`.
- **Detail**: See Approach, "Proving". The script uses the `p` helper shape from `claude/skills/flow-contract-plan-output-format/references/acceptance-probing.md`, keyed `<task id>:<accept index>`, with neg and optional pos lines, but carries stderr whole with newlines folded rather than cut to 100 bytes; quote every command safely, testing embedded single quotes. Rows whose `writes_build` is not explicitly `false`, and rows with an empty `check`, are skipped and listed in a comment, so an unlabelled command never runs. tomlctl spawns nothing: the script's first output line is `head<TAB><sha from git rev-parse HEAD>`, and `--record` reads the script's captured stdout, that line plus the helper's five tab-separated columns, into `probe.at`, `neg_rc`, `pos_rc` and `verdict`. Verdicts, from exit status: forward is `healthy` when neg fails and pos passes or is absent; forward that already holds is `vacuous`; forward with a failing pos is `unsatisfiable`; falsifier and guard must hold at neg, and a falsifier that does not is `permanently-green`; stderr naming `command not found` or `fatal`, or `No such file` for a path no row's `files` declares, is `broken`. A guard beside no discriminating check on the task records `guard-alone`. Put a `git_bash()` helper in `tomlctl/tests/common/mod.rs`, located as `bash_candidates` in `tomlctl/src/cli/dispatch/tests/skills.rs` does because a bare `bash` on Windows resolves to System32's WSL launcher, and run the quoting test through it.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_probes` passes **predicted, unverified**; falsifier: treating a passing forward as healthy turns the vacuous case red.

#### 17. Implement plan stage [M]
- **Files**: `tomlctl/src/plan/stage.rs`, `tomlctl/tests/plan_stage.rs` (new)
- **Depends on**: 11, 12, 13, 14, 16
- **Action**: Implement `plan stage`, computing `{stage, next, blockers}` from artefacts with nothing stored.
- **Detail**: Stages: `open` (brief incomplete), `design` (no milestones or tasks), `write` (a task lacks a spec), `prove` (check errors, or a probe absent on an `accept` with a non-empty `check` and `writes_build = false`), `review` (no finished plan-family run in `review-v2.toml`, and no `[[run]]` with `step = "review"` and a `skip_reason`), `ready`, `approved` (an `[[approval]]` exists and every row is pending), `running` (a row is past pending), `done` (every row is done). Read `review-v2.toml` as a plain document for a finished run, since Slice 3 owns the rest. `next` names the next verb, not a procedure. No verb gates on this output.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_stage` passes **predicted, unverified**; falsifier: a parametrised case per stage turns red if any artefact is ignored.

#### 18. Implement plan gate and approve [M]
- **Files**: `tomlctl/src/plan/approve.rs`, `tomlctl/tests/plan_approve.rs` (new)
- **Depends on**: 2, 3, 11, 12, 13, 14
- **Action**: Implement `plan gate` and `plan approve`, per Approach, "Approval".
- **Detail**: Approve refuses on any `plan check` error or a surfaced decision still `agent-assumed`, and when `HEAD` does not contain the flow directory's current `tasks.toml`, `plan.toml` and `brief.md` content, so the stamp cannot name a commit lacking the plan. Ask git rather than comparing bytes, since the working copy's line endings need not match the blob's: require empty `git status --porcelain -- <paths>`, because `git diff --quiet HEAD -- <path>` exits 0 on an untracked file. Test that a never-committed `plan.toml` is refused. Stamp the brief through `brief_digest`. Promote `closes` backlog links through the same library path as `backlog triage`, with `--expect-status open` semantics, reporting skips. `--amend` appends and refuses without a prior approval. Status moves through `crate::io::mutate_doc` on `context.toml`, changing only `status`, `branch` and `scope`, which keeps `created`. The envelope reports `transition: {from, to}`, since the `status-transition` record is the carrier's to append through `flow record`. `plan gate` follows the order in that section; review-derived entries are empty until Slice 3 and say so.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_approve` passes **predicted, unverified**; falsifier: approving with an unanswered surfaced decision must be refused, so removing that check turns it red.

#### 19. Implement plan drift [S]
- **Files**: `tomlctl/src/plan/drift.rs`, `tomlctl/tests/plan_drift.rs` (new)
- **Depends on**: 18
- **Action**: Implement `plan drift`: files changed from the last approval's commit to `HEAD`, minus commits recorded on the store's rows, intersected with each pending task's `files`, reported per task with its premises.
- **Detail**: Warning only, exit 0, because replaying over real histories comes before any blocking use. Shell out to `git` as `tomlctl/src/repo_files.rs` does. No approval is a `kind=validation` refusal.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_drift` passes **predicted, unverified**; falsifier: not subtracting recorded commits turns the own-commit case red.

#### 20. Add the lifecycle test [M]
- **Files**: `tomlctl/tests/plan_lifecycle.rs` (new), `tomlctl/src/lib.rs`
- **Depends on**: 4, 5, 6, 10, 13, 15, 16, 17, 18, 19
- **Action**: Drive one plan end to end through the binary against a temporary git repo, and remove the `#[allow(dead_code)]` task 7 left on the `plan` module declaration in `tomlctl/src/lib.rs`.
- **Detail**: Open from a backlog row with a scope; assert `flow resolve --path` picks no draft; decide with a surfaced decision; add a contract and a handover; start and finish a run; skip the review step; add milestones, one deferred; `tasks add-many` with specs, a `closes` link and a `(new)` file note from a TOML record file; run the emitted probe script with the `git_bash()` helper from `tomlctl/tests/common/mod.rs`, skipping if none is found; record; check; `plan view` for each audience; `plan gate`; approve; assert the backlog row reads promoted and the registry entry and scope are in place; `tasks update` with and without a reason; `tasks remove`; commit a change to a task's file; `plan drift`; assert `plan stage` at each point. Assert `tasks import-plan` is refused. This is the test the Success Criteria name, and it is the one place the record shape meets a realistic sequence before a carrier exists.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_lifecycle` passes **predicted, unverified**; falsifier: any verb returning the scaffold's stub error turns it red.

### Phase 5: Documents and closure

#### 21. Document the plan group [M]
- **Files**: `claude/skills/tomlctl/references/plan.md` (new), `claude/skills/tomlctl/SKILL.md`, `tomlctl/src/cli/dispatch/tests/finding_classes.rs`
- **Depends on**: 20
- **Action**: Write the `plan` reference (flag tables, store shape, check classes, output shapes) and add its Quick Reference row and References entry.
- **Detail**: `flag_table_lint` and `command_lint` under `tomlctl/src/cli/dispatch/tests/lint.rs` check these against the real CLI, so write them from the built verbs. Bodies stay under the 600-line reference ceiling. Say what the fields mean and leave orchestration to Slice 2's engine skill. Add `plan.toml` to the verb-owned stores bullet in `claude/skills/tomlctl/SKILL.md`. Add a second pair to `tomlctl/src/cli/dispatch/tests/finding_classes.rs`, `src/plan` against `references/plan.md`, rather than widening the first, which would make `tasks.md` report the new classes as missing.
- **Acceptance**:
  - forward: `test -f claude/skills/tomlctl/references/plan.md` exits 0 (today: exits 1).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes **predicted, unverified**.

#### 22. Document the task-store additions [S]
- **Files**: `claude/skills/tomlctl/references/tasks-store.md`, `claude/skills/tomlctl/references/tasks-write.md`, `claude/skills/flow-contract-task-store/SKILL.md`
- **Depends on**: 2, 3, 5, 6, 9
- **Action**: Document `[[specs]]`, `[[milestones]]`, `[[changes]]`, `audit`, the conditional `schema_version = 2` stamp, the `spec`, `reason`, `actor`, `file_notes`, `closes` and `refs` row keys, `--reason`, `tasks show --with spec`, and the v2-flow refusal.
- **Detail**: Update the task-store skill's field table, the `add-many` accepted-key list in its section 5, and the side-table count in its section 5a. Keep v1 semantics stated as unchanged.
- **Acceptance**:
  - forward: `grep -c 'specs' claude/skills/flow-contract-task-store/SKILL.md` prints at least `1` (today: `0`).

#### 23. Exempt plan cleanup rows from old [S]
- **Files**: `tomlctl/src/review/fields.rs`, `tomlctl/src/review/schema_out.rs`, `tomlctl/tests/review_add.rs`
- **Depends on**: —
- **Backlog**: B-ef306556
- **Action**: Make `old_required` return false for the plan family, and add test `plan_cleanup_row_needs_no_old`.
- **Detail**: `claude/agents/review-sweeper.md` step 8 already documents plan rows as anchor-only: `plan_section`, `anchor_old`, `anchor_new`. `old_required` takes `ctx`, whose `family` decides it. Code rows keep their rule. `conditionals` in `tomlctl/src/review/schema_out.rs` sees only the kind and a row carries no family, so its `if`/`then` for `Need::OldText` would refuse a valid plan row: remove that rule, leaving `validate_row` to enforce it, reword the matching note to except the plan family, and update any inline test pinning either. Task 28 carries the documents. Slice 3 replaces plan anchors with record targets; this rule stays correct until then.
- **Acceptance**:
  - forward: `grep -c 'plan_cleanup_row_needs_no_old' tomlctl/tests/review_add.rs` prints `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test review_add` passes **predicted, unverified**; falsifier: removing the family check turns the new test red.

#### 24. Bump the tomlctl version [L]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `tomlctl/README.md`, `tomlctl/tests/capabilities.rs`
- **Depends on**: 20
- **Action**: Bump tomlctl to the next minor version and refresh both lockfiles.
- **Detail**: Run `cargo tree --manifest-path glimpse/Cargo.toml >/dev/null` to rewrite the glimpse lockfile, because the pre-commit hook's `--locked` clippy refuses a stale one. Derive the new version from the current one, a new verb group being a minor bump. Update the version literal and its message in `tomlctl/tests/capabilities.rs`, and the sample version in `tomlctl/README.md`, which `readme_sample_version_matches_cargo_toml` compares. Inseparable: the suite goes red until all three agree.
- **Acceptance**:
  - forward: `grep -c '^version = "0.17.0"' tomlctl/Cargo.toml` prints `0` (today: `1`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes **predicted, unverified**; falsifier: leaving the literal in `tomlctl/tests/capabilities.rs` at the old version turns it red.
  - guard: `cargo clippy --manifest-path glimpse/Cargo.toml --locked` passes **predicted, unverified**.

#### 25. Bring the idea doc into line [S]
- **Files**: `docs/ideas/v2-pipeline.md`
- **Depends on**: —
- **Action**: Mark topic 2 decided by this plan (topic 1 already reads Decided), replace the "Planning flow: draft" section with a pointer to this plan's Approach, and correct the decisions in Approach, "What departs from the idea doc".
- **Detail**: The file is currently untracked, so the commit adds it whole. Keep its Evidence section, adding this plan's corrections: line 194's SkillsBench reading (comprehensive n=5, standard from a 19.8% baseline, no Haiku data) and line 206's 794 of 810 being a product of the merge path. Facts belong to one file, so point at the plan's Approach rather than restating it. Leave topics 4 to 8 open.
- **Acceptance**:
  - forward: `grep -c 'Planning flow: draft' docs/ideas/v2-pipeline.md` prints `0` (today: `1`).

#### 26. Pin flow stores to LF and repair their sidecars [L]
- **Files**: `.gitattributes`, `.claude/flows/binary-jumping-yao/context.toml.sha256`, `.claude/flows/binary-jumping-yao/execution-record.toml.sha256`, `.claude/flows/flow-tracking-overhaul/context.toml.sha256`, `.claude/flows/flow-tracking-overhaul/execution-record.toml.sha256`, `.claude/flows/full-slice-flow-dogfood/context.toml.sha256`, `.claude/flows/full-slice-flow-dogfood/execution-record.toml.sha256`, `.claude/flows/groovy-growing-yeti/context.toml.sha256`, `.claude/flows/groovy-growing-yeti/execution-record.toml.sha256`, `.claude/flows/lumina-story-planning-round-4/execution-record.toml.sha256`, `.claude/flows/lumina-story-planning-round-5/context.toml.sha256`, `.claude/flows/lumina-story-planning-round-5/execution-record.toml.sha256`, `.claude/flows/lumina-story-planning-workflow/execution-record.toml.sha256`, `.claude/flows/lumina-vertical-slice/context.toml.sha256`, `.claude/flows/lumina-vertical-slice/execution-record.toml.sha256`, `.claude/flows/snoopy-beaming-lecun/context.toml.sha256`, `.claude/flows/snoopy-beaming-lecun/execution-record.toml.sha256`
- **Depends on**: 21, 22, 24, 25, 28, 29
- **Backlog**: B-33df5eda
- **Action**: Pin `.claude` stores to LF, rewrite the CRLF working copies, and refresh the sidecars that were stamped against CRLF bytes.
- **Detail**: This task runs last, with nothing else in flight, and commits alone, because step 2 leaves no diff to roll back. Step 2 is licensed for the delegate: its `git diff --quiet` guard discards no uncommitted work, so it is not a `stash-required` case. Inseparable: the pin alone rewrites nothing, and rewriting alone fails the 16 sidecars listed above, whose committed digests match CRLF bytes rather than the LF blobs.
  1. Add to `.gitattributes`, with a why-comment in that file's style: `.claude/flows/** text=auto eol=lf`, `.claude/**/*.toml text eol=lf` and `.claude/**/*.sha256 text eol=lf`. `text=auto` keeps a binary later added to a flow from being converted. Markdown under `.claude/plans/`, `.claude/rules/` and `.claude/skills/`, and `.claude/settings.local.json`, stay unpinned; TOML and sidecars under `.claude/plans/` and `.claude/reviews/` are pinned, which is what repairs them. The patterns were tested with `git check-attr` in a scratch repo.
  2. For each path from `git ls-files --eol .claude` reading `w/crlf` whose `git check-attr eol` is `lf`: skip and report it if `git diff --quiet -- <path>` fails, otherwise `rm <path>` then `git checkout -- <path>`. A plain `git checkout` leaves the file CRLF, because git sees it as unchanged; this was tested in a scratch repo. These rewrites produce no diff, since the index is already LF, which is why those files are not on the **Files** line.
  3. For each `.toml` step 2 rewrote, with a sidecar whose digest now differs from the file: `tomlctl validate <path>` then `tomlctl integrity refresh <path>`. This leaves `.claude/flows/lumina-schema-deepening/plan-review-findings.toml` alone: it is already LF, its sidecar matched neither its LF nor its CRLF bytes when measured, and refreshing would bless an unexplained mismatch, so report it. Compare the refreshed set against **Files**. A path outside it is stop-and-report, because it means a digest matched neither form when measured.
  Never run step 3 before step 2: refreshing against CRLF bytes is what caused the 16. A rollback that restores only **Files** leaves LF stores against CRLF digests, so rerun step 3 after one.
- **Acceptance**:
  - forward: `git check-attr eol -- .claude/flows/x/brief.md` prints `.claude/flows/x/brief.md: eol: lf` (today: `.claude/flows/x/brief.md: eol: unspecified`).
  - forward: `git ls-files --eol .claude/flows | grep -c 'w/crlf'` prints `0` (today: `105`).
  - forward: `tomlctl flow doctor --rows checks --where ok=false --where-suffix name=-sidecar --get name` prints nothing (today: 12 lines).

#### 28. Say plan cleanup rows carry no old [S]
- **Files**: `claude/skills/tomlctl/references/review.md`, `claude/skills/review-v2-method/SKILL.md`, `claude/agents/review-sweeper.md`
- **Depends on**: 23
- **Action**: State the plan-family exemption from `old` in the three places that describe the requirement.
- **Detail**: In `claude/skills/tomlctl/references/review.md` and `claude/skills/review-v2-method/SKILL.md`, add the words "except on a plan review" to the statement requiring `old` for `change` and `remove`, which is what task 23 makes `old_required` do. In step 8 of `claude/agents/review-sweeper.md`, add "and carry no `old`" to the sentence listing what plan rows carry. Step 8 sits outside the shared blocks that file carries.
- **Acceptance**:
  - forward: `grep -c 'except on a plan review' claude/skills/tomlctl/references/review.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'except on a plan review' claude/skills/review-v2-method/SKILL.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'carry no' claude/agents/review-sweeper.md` prints `1` (today: `0`).

#### 29. List plan.toml among the owned stores [S]
- **Files**: `claude/skills/tomlctl/references/write.md`
- **Depends on**: 8
- **Action**: Add a `.claude/flows/<slug>/plan.toml` row, with `plan` as its owning verb, to the owned-store table in `claude/skills/tomlctl/references/write.md`.
- **Detail**: Task 8 makes `plan.toml` an owned store, and this table enumerates them. Follow the row for `.claude/flows/<slug>/tasks.toml`.
- **Acceptance**:
  - forward: `grep -c 'plan.toml' claude/skills/tomlctl/references/write.md` prints `1` (today: `0`).

### 30. Hold plan out of the output-options leaf gate [S]
- **Files**: `tomlctl/tests/output_options.rs`
- **Depends on**: 9
- **Action**: Exclude `plan` leaves from `every_leaf_command_has_a_case` while the group's verbs are stubs, because a stub's validation error cannot pass a case and every checkpoint gate runs this suite.
- **Acceptance**: forward: `cargo test --manifest-path tomlctl/Cargo.toml --test output_options every_leaf_command_has_a_case` passes; falsifier: removing the exclusion turns it red, naming the plan leaves.

### 31. Give every plan verb an output-options case [M]
- **Files**: `tomlctl/tests/output_options.rs`, `tomlctl/src/plan/stage.rs`, `tomlctl/src/plan/approve.rs`, `tomlctl/src/plan/drift.rs`
- **Depends on**: 15, 17, 18, 19, 30
- **Action**: Add a case for each `plan` leaf to `cases()` in `tomlctl/tests/output_options.rs`, add a `plan` group to `group()` and `group_tests!`, and remove the `listed.retain` exclusion from `every_leaf_command_has_a_case`.
- **Acceptance**: forward: `cargo test --manifest-path tomlctl/Cargo.toml --test output_options` passes with the exclusion gone; falsifier: deleting any one plan case turns `every_leaf_command_has_a_case` red, naming that leaf.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 3, 5, 27 — dependency closure: 1, 2, 3, 4, 5, 6, 27. Baseline repair and task-store additions, with v1 behaviour unchanged (buildable increment)

— CHECKPOINT B after tasks 8, 10, 30 — dependency closure: 7, 8, 9, 10, 30. Plan store, guard and the stubbed command group (buildable increment)

— CHECKPOINT C after tasks 11, 15 — dependency closure: 1, 2, 7, 8, 9, 11, 12, 13, 14, 15. Authoring verbs and the views over them

— CHECKPOINT D after tasks 20, 31 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 30, 31. Proving, approval and the lifecycle test

— CHECKPOINT E after tasks 21, 22, 24, 25, 28, 29 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 28, 29. Documents, the backlog item, version and the idea doc

— CHECKPOINT F after tasks 26 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 28, 29. Line endings, alone, because step 2 leaves no diff to roll back

## Risks

- **The record shape is fixed before any carrier uses it.** Mitigation: the lifecycle test drives a realistic sequence, every addition is additive, and Slice 2 can still change records since no real plan exists yet.
- **v1 regression through shared task-store code.** Mitigation: side tables omitted while empty, a byte-identical round-trip test, and the existing `tasks_*` suites at checkpoint A.
- **The structure challenge may add cost and no signal.** The evidence is adjacent. Mitigation: replay over past goals opens Slice 2, and the step has an off-switch.
- **Surfacing by judgement may under-escalate,** since an agent's own decisions to ask reached only 34.6% recall on HiL-Bench's software tasks. Mitigation: the three tests are concrete, `agent-assumed` decisions stay visible at approval, and `vetoed` gives a measure. Dogfooding decides whether a stronger prompt is needed.
- **Plan-level contracts rest on low-grade evidence.** Mitigation: optional records, watched in early plans.
- **`plan approve`'s committed-content check may annoy,** because it requires committing the flow's stores, brief and design files first. Mitigation: the refusal names the files, and the carrier commits as a step in Slice 2.
- **`--emit` scripts carry stored strings into Bash.** The harness's permission prompt does not review them: Bash rules match command text, a prompt would show `bash <file>`, and under `defaultMode` `auto` the classifier reviews the call instead of a prompt. A stored `check` is repo code run with the user's rights, on the same terms as a `success:` key, and only rows marked `writes_build = false` run. Quoting is tested in task 16.
- **Task 26 rewrites other flows' working copies and refreshes their sidecars.** Mitigation: it runs last and commits alone, skips any path with real local edits, validates each store before refreshing it, and stops on any sidecar outside the measured 16. A set that drifted before execution shows up there.
- **A new session reruns a whole Workflow step unless the carrier narrows it.** The [docs](https://code.claude.com/docs/en/workflows.md) ("Resume after you leave the session", fetched 2026-11-06) settle the runtime: a run does not continue on its own after the session leaves, and continues only through a relaunch. Interruption is resumable in the same session, `claude --resume` included, but a fresh session has no earlier run to relaunch and starts over. Mitigation: a script has no filesystem, so each Workflow takes a work-list in `args` (pending lenses, ready tasks) that the carrier computes from `notes/` or the store before launch, and the store acts as the journal; `[[run]]` records are written at launch. Slice 2 runs three tests before relying on this: kill a session mid-run, launch again from a new session, and confirm only pending work reruns; under `claude --resume`, relaunch after editing a file a prompt only points at, and confirm the cached result is stale; repeat with a changed first prompt, and confirm a miss from there on.

## After Merge

- Run `cargo install --path tomlctl` and `cargo install --path glimpse`, since glimpse links its own copy of tomlctl. An older installed binary refuses a v2 store, so reinstall in any other clone before it touches one.
- Decide what to do with `.claude/flows/lumina-schema-deepening/plan-review-findings.toml`, which task 26 reports and leaves alone: its sidecar has matched neither its LF nor its CRLF bytes since `fb10afc`.
- Update the memory note `v2_pipeline_direction` to record the design's changes, and point it at `docs/plans/v2-planning.md`.
- Run `/plan-new` for Slice 2, taking this plan's Approach as input.
- In any other clone or worktree with `core.autocrlf` on, repeat step 2 of task 26. Its CRLF copies stay CRLF until they are removed and checked out again.
- Run the **predicted, unverified** criteria by hand if `/implement` Phase 3 did not cover them.

## Exploration Notes

Checkpoint from three Explore sweeps, 2026-11-05. Paths are repo-relative.

### v1 planning surface

- `tomlctl/src/tasks/` is 16,263 lines. Round trip group is 6,682 (`import_plan` 2447, `parse_tasks` 1790, `render` 1314, `parse_policy` 762, `markdown` 369). It is not cleanly removable: `parse_policy` feeds `train`, `check`, `cli/types/tasks.rs`; `markdown` feeds `flow/resolve.rs`; `import_plan`'s resolvers feed `flow/doctor.rs`, `flow/resolve.rs`. Only `parse_tasks` is exclusive.
- `check` has 31 classes, 13 markdown-only (`plan/*` 11, `checkpoint/marker-mismatch`, `render/drift`).
- Consumed fields: `status`, `needs`, `files`, `effort` (lite gate), `checkpoint`, policy, `origin`, `commit`, `ref`, `file_notes`, `backlog_links`. Prose fetched by agents: `action`, `detail`, `acceptance`. Display only: `phase`, `phase_depth`, `heading_depth`, `deps_note`, `*_note`, group `rationale`. Bookkeeping: `last_import_refs`, `import_overrides`. Unwritten: `coupling`.
- **Acceptance is never machine-run.** No `tasks` module parses `forward:`/`falsifier:`/`guard:`/`today:`. `/implement` gates run `checkpoint:`, `build:`, `test:`, `success:`; lite vet compares against Action+Detail, not Acceptance.
- Notes appendix is never sent to implementers; only `/plan-update reformat`/`catchup` read it. `Affected areas` is used once, for `scope`.
- `/review-plan` mutates via 4A (overwrite + `.premerge.md`) or 4B (`anchor_old`/`anchor_new` into `.revised.md`), then re-imports.
- Pain: parser silently lost claims or edges (`B-7b40308f`, `B-47118e1e`, `B-22bd6fc6`, `B-83555773`, `B-47926bdb`, `B-a6c92b55`); `B-da4e2b62` clean check with 4 missing edges; `B-d17eff50` Files omissions were the commonest review defect. `docs/ideas/plan-flow-mechanical-verification.md`: review merge is an unreviewed write (727 to 1073 lines, created the only file-claim collision); completeness lens missed 12 files, 7 prose; implement delegates were the high-recall detector; `files/closure` flagged at 68% as a gate.
- Reusable: `tasks ready` (ready/held/next/blocked with file claims), `train`, `closure`, `show --with absent`, fetch-by-id dispatch, `add-many` validator, two-control probe helper and verdict table, vet pass, `Searched:` + Counter, `flow init` idempotency, `--expect-status`, derived writes without sidecars, info severity for noisy heuristics.

### Built v2 and Workflow substrate

- `review-v2-pipeline` (`claude/workflows/review-v2.js`, name from `meta`): args `{run, store, ledger_line, wave, depth, pack_path, cells, schemas}`; returns index `{run, wave, cells, verified, errors, script_version}`; agents write the store. Callable as child via `workflow(...)`, one nesting level, shared cap and budget; unused so far. No `resumeFromRunId` wiring; recovery is same-args relaunch + idempotent writes.
- Workflow runtime (`workflow-authoring` skill): no user hook, no filesystem from the script, no `Date.now`; only agents touch disk; deterministic resume from the journal; `agent(..., {agentType, schema, effort, model, isolation})`.
- `/review-plan-v2` binds one markdown path: `store-check` runs `import-plan --dry-run` + `tasks check --plan`; `format-contract` greps headings; `plan-executor` reads preamble headings; findings need `plan_section` and `anchor_old` (`add.rs::check_anchor_old`). `B-ef306556` open on old/anchor_old. Probable bug: `plan-executor.md` assumes a plan path no cell field carries. A flow-directory plan needs a new input binding, record targets, and tool-applied edits.
- Store precedent `tomlctl/src/review/`: field catalogue in `fields.rs` drives `validate_row`, `project`, `schema_out`; typed `store.rs` with `rest` passthrough; ids minted under lock; `digest`; `--slug|--scope`; replay-safe writes; `--*-file` twins; `--expect-status`.
- Flow state: `flow init --plan <.md>` required; resolve order flag, plan-path-arg, scope-glob, active-binding, active-latest, branch-match, none. Only `is_complete` filters plan-path/scope/branch; drafts win active-binding, and active-latest ignores status. `FLOW_STATUSES` refuses a new status; unknown reads as in-progress. Seed flows are the hidden-draft precedent (`flow init` then `flow active remove`).
- Task store: `[[checkpoints]]` written only by `import-plan`; `tasks add --checkpoint` refuses undeclared groups; `add-many` rejects unknown fields; glimpse `model.rs` has `files: Vec<String>`, conflicts with `{path, change}`.
- Backlog: `summary`, `context`, `evidence` (pointers; files in git-ignored `.claude/backlog-evidence/<id>/`) exist. `done_when`, `questions`, `route`, `shape` do not; `add --json` refuses unknown keys. Promote accepts drafts, refuses `review`/`complete`. `reconcile` joins through `[[backlog_links]]`, which only import rebuilds, or `--adopt`.
- glimpse: lists flows with `tasks.toml` or a v1 ledger (`SNAPSHOT_INPUTS`); shows draft rows and `flow_status`; not `brief.md`, `design/`, approval state; no `review-v2` reference. Attribution reads a column-0 `ledger:` line with a fixed filename list (`tomlctl/src/agents/correlate.rs`).
- Approval rule: `flow-contract-user-inputs` says a record is never approval for something a carrier would otherwise ask. Revisit if glimpse approves.
- Gates: `tomlctl/src/cli/dispatch/tests/` `lint.rs` (`command_lint` incl. `claude/workflows/*.js`, `guidance_lint`, `flag_table_lint`), `skills.rs` (`carrier_invokes_required_skills`; 500-line bodies, 600-line refs, 1024-char descriptions), `store_guard_gate.rs` (`OwnedStore`), `finding_classes.rs`, `catalogue.rs`; `scripts/shared-blocks.toml`; `flow/envelope.rs::VALID_COMMANDS`/`VALID_ARTIFACTS`; `flow/artifacts.rs` `[..; 5]`; `capabilities.rs::MUTEX_GROUPS`.
- Deploy: `deploy-claude.ps1 -Check` reports 15 entries missing, including all v2 agents, commands, skills, and `workflows/`.
- Conflict: idea doc wants no Agent fallback while the engine keeps one and dispatches the surveyor through Agent; `plan-review-lead` reverses the engine's orchestrator-composes rule.

### Lumina planning model

Paths: `lumina/core/migrations/`, `lumina/core/src/`, `claude/plugins/lumina-story-blocks/` (`skills/plan-story/SKILL.md:130-380`, `CONVENTIONS.md` §o :427-539, §k.1 :287-302). No retrospective exists; outcomes from `lumina.db` activity rows read with `immutable=1` (5 MB WAL ignored, counts may be low).

- Stages: `triage -> frame (grill 1) -> plan (phases 2-5, auto) -> brief -> align (grill 2) -> rework`; at most one mid-plan interrogation per pass. Gating rule `repo/task_graph.rs:144`: autonomous/full/light; changes interaction, not depth. No real depth scaling exists.
- Lenses: `codebase`, `library`, `risk`, `completeness`, `contrarian` always; `domain` on high complexity. Notes start `proposed`; only `vet-research` promotes.
- Worth reusing: two concentrated grills instead of per-block gates; five-heading brief (Problem, Chosen approach with competition, Impact, Grounding, Alignment questions); approach tournament on four axes with losers recorded; contrarian demanding a cited counter-example; rework routed by disagreement type (scope to frame, approach to decide, decomposition to decompose); grills degrade to durable questions; pure-function tier rules with per-branch tests; candidate edges from file overlap then pruned.
- Outcomes. Overhead: per-block gate fired ~16 times, extracted nothing. Caught: 1B-F12 `story-review` critical on stash/pop re-introducing the merge reversion, align grill then forced rework; 1C.3 contrarian raised ADR-0006 regression and informed but did not decide. Usage: 21 legacy walks, 2 orchestrated.
- Weak or unused: triage tier on thin inputs (both `light`, one moved to `full` after decomposition); child epoch stamps 0/696; the only real rework recorded no supersessions and re-specced in place; option-to-task branches 0 rows of 41 answered questions; supersession 7/353 notes, 0/71 risks, 0/54 alternatives; vet 9 rejected against 281 accepted; complexity set on 0/38 stories.
- Dogfood defects: full-story reads overflow ~60k cap (1A-F16); deep tasks bloated and an invariant fell between two tasks (1A-F17); tasks done with 8/8 story criteria unchecked because the gate counted the wrong level (1B-F7).
- Constraint: phase preconditions are prose, not enforced.

### Backlog

Live rows under the touched directories, all `open`, none promoted. Relevant: `B-ef306556` (plan cleanup rows need old and anchor_old; subsumed if anchors become record targets), `B-c5783e08` (research vet_events stamping), `B-b9b7e4c1` (classifier direction; context only), `B-7b662012` (mini layer, seed-plan; context only). Others are review-v2 or glimpse-diagram detail: `B-e4c1a22a`, `B-13ec633a`, `B-8414a070`, `B-c6fcfb97`, `B-e922f5cb`, `B-771b57e7`, `B-317ed123`, `B-29b27f75`, `B-7ffd9652`, `B-8d86ceb4`, `B-a0ce71a9`.

### Scope flag

Designing plus building this likely exceeds ~25 files across tomlctl, glimpse, carriers, skills and agents. Expect a split recommendation.

## Research Notes

Four `research-deep` lenses, 2026-11-05 (departs from the command's 2-agent Phase 3; stays within its total budget of 8 by skipping Plan agents). Each drafted external findings before reading the idea doc's decisions. Most sources are 2026 preprints; nothing tests this exact setting.

**Vet outcome** (no ledger exists in plan mode, so this stands in for `[[vet_events]]`):

| Lens | Sampled | Dropped | Downgraded | Note |
|---|---|---|---|---|
| pipeline-practice | 4 | 0 | 1 | Oota's task-blind null confirmed; "permutation" not seen in text read |
| decomposition-and-handoff | 5 | 0 | 1 | Idle-compaction claim holds on the one-hour cache only; TDFlow 93.3% is the all-valid-tests subset |
| decisions-and-gates | 4 | 0 | 1 | Self-attribution AUROC figures hold, "Sonnet 4" attribution unsupported; "5x" is one setting; `review-plan.md` merge path and 12-open count confirmed |
| plan-state-and-orchestration | 3 | 0 | 0 | 50/2/1/1 flow statuses, 7-day hardcoded threshold, dropped unknown keys confirmed; recall result confirmed (Nemotron and Mistral harness, not Claude) |

Ask or Assume 61.2/54.8 is not a conflict: 54.8 is no contact, 61.2 single-agent clarify, 69.4 separate detector, 70.4 one batched round, 70.8 full issue (Sonnet 4.5, simulated user).

### pipeline-practice

Searched: SDD and tool surveys, plan quality, plan vs no plan, adaptive depth, misrouting, subagent exploration; newest source 2026-11-03.

- **Recurring core is small.** Intent capture, code research, a written plan, fresh-context execution, external verification recur. Separate requirements docs, constitutions, judges, integrators, sprints get removed (Cursor removed integrator and judge; Anthropic removed sprints). Temperature-0 runs flip 9% of outcomes, so stage comparisons need many runs. Impact: every stage has an off-switch, a recorded cost, a stated assumption.
- **Plan benefit depends on executor and planner strength.** [Fan 2609.20804](https://arxiv.org/html/2609.20804): weakest model 13.6 to 25.2 with planning; two strongest lose 0.4 and 2.0 at 30-32% lower cost. DCAS 2608.06113: fixed executor 42.8 no plan, 57.8 stronger planner. Impact: prescriptive effort goes to cheap tiers; `deep` gets outcome and constraints.
- **Plans are not commitments.** [Oota 2609.38108](https://arxiv.org/html/2609.38108v1): 22-45% of trajectories preserve declared structure; judged plan quality weakly tied to success, |r| <= 0.181. SWE-RPG: planning coverage 79.7% on location, 41.6% on constraints and validation. Impact: aim checks at constraints and validation.
- **Default-on review has no outcome evidence.** [Abdo & Brom 2611.05094](https://arxiv.org/html/2611.05094): re-reviews 75.3% of reviewer tokens, no control. Kiro and GSD bound or skip it. Impact: default by risk; log outcome joins.
- **No replanning owner in the draft.** Line 65 sends failures "back for replanning" while lines 33, 40, 139 remove `/plan-update`, freeze the plan and digest at the end. [Cursor](https://cursor.com/blog/self-driving-codebases): "Doing all planning upfront also made it hard for the system to dynamically readjust". ADaPT and Plan-and-Act show replanning gains in non-coding domains. Devin weak on mid-task change supports freezing per task, not across tasks.
- **Decide outcomes upfront, defer means.** Anthropic constrains planners to high-level design because spec errors cascade; BMAD refines thin epics; Codex prototypes milestones. Counter: Superpowers requires complete code in each task with self-reported good results; parallel dispatch needs file sets early. Unmeasured at 5-60 tasks.
- **Self-scaling is the weakest form.** Oota: no task-specific advantage from model-chosen mode. E3: Expand matters more than Estimate; lexical estimator 85% to 67% on paraphrases. Kiro, BMAD, Codex, Claude Code leave depth to the human. Impact: start minimal; let observed results raise depth; user override; numeric ceiling.
- **Index for finding, source for deciding.** [2607.09691](https://arxiv.org/abs/2607.09691): summaries answered 4/45 behavioural questions against 27/45 for source. Impact: index entries carry `file:line`; the orchestrator reads source for design-bearing facts.

### decomposition-and-handoff

Searched: handoff failures, merge conflicts, TDD with agents, oracles, EARS, premise benchmarks, contract-first; newest source 2026-09-26. No study found on spec quality late in a session, orchestrator vs fresh-writer authoring, prescription by tier, EARS in agentic coding, premise checks, interface-first decomposition, milestone derivation.

- **Author by where the information lives.** [SkillsBench](https://arxiv.org/html/2602.12670): self-generated skills -8.1 to -11.5pp, curated +18.2 to +24.8pp (three harness configurations; effort caveat). AGENTS.md study: LLM-written context no uplift. Cognition: actions carry implicit decisions. [Ask or Assume](https://arxiv.org/html/2603.26233): separate read-only detector 61.2 to 69.4. Impact: "fresh cold readers challenge" replaces "fresh writers fill in".
- **Context budget does not justify delegation.** Largest plan's task text about 27k tokens, about 3% of 1M. [Opus 5 guide](https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/prompting-claude-opus-5) asserts consistency through the window (vendor, not repeated on the 5.5 page). [Idle compaction](https://code.claude.com/docs/en/best-practices) past about 200K on the one-hour cache is the real risk. Impact: write decisions to files before task writing; write tasks in one pass.
- **`files` is the least reliable input.** `cross-cut` is "needs a file outside the set" (`implement-lite.md`), 61 of 82 escalations. Conflict prediction F1 0.57-0.68. Impact: file overlap gives serialisation edges only; semantic edges are declared; widen by computed closure; shadow-log eligibility first.
- **No cross-task object.** [CooperBench](https://arxiv.org/html/2601.13295): about 30% worse together; 42/32/26 expectation/commitment/communication; communication cut conflicts, not failures (no orchestrator, isolated workspaces: direction, not magnitude). Remedy low-grade. Impact: plan-level contracts checked at the join; interface tasks first; coupled edits stay together.
- **Acceptance intent comes from the context holder.** [TDFlow](https://arxiv.org/html/2510.23761v2): 94.3 human tests, 68.0 LLM tests, 93.3 when all generated tests valid. Konstantinou: LLM oracles capture actual over intended behaviour. METR: merges 24pp below grader passes. Impact: orchestrator writes `behaviour`; a fresh agent may materialise `check`, not change it.
- **Label is not power.** 80.2% of agent test patches have weak oracle signals. Meta ACH mutants accepted 73%. Impact: tool runs the check at base, requires the expected failure, re-runs after; sampled hunk reversal is a hypothesis to test on past tasks. No EARS.
- **Premises catch staleness only.** A premise written by a reader of current code is true by construction. Impact: `at = base | after:<id>`; a false premise goes to replan, never up a tier. Low grade.
- **Prescription.** Sonnet 5 does not generalise an instruction from one item to another; Haiku 5.5 stops early at low effort; Opus 5 can expand scope. Impact: `means` empty by default, enumerated targets for clerks, scope clause for deep. Line 74's one-line Haiku swap conflicts with the guide.
- **Idea-doc check.** Lines 194-197 numbers correct but 194 over-read: comprehensive n=5, standard starts from 19.8%, no Haiku; omits the self-generated result.

### decisions-and-gates

Searched: clarification benchmarks, approval fatigue, pre-mortem, sycophancy, ADR studies, LLM over-rejection; newest 2026-10. No study found on expert attention per question, expert plan approval, LLM pre-mortems or assigned contrarians, agents honouring rejected alternatives, decision-log readership.

- **Admission test for questions.** ClarifyGPT, CONTRA keep a question only if alternatives behave differently. Ask or Assume: one batched round recovers nearly all (70.4 vs 70.8). Benchmarks delete information; internal failures are wrong or over-specified. Impact: each question carries "if A, X; if B, Y", one batch, with a default; the rest become disclosed assumptions. Remove the review opt-out question.
- **Gap between stages 3 and 8.** Design choices first reach the user at approval, after tasks and review are spent. Drops Lumina's align grill. Menzies: no general delayed-issue cost, so the case is wasted agent work and a large decision at a cheap-yes gate. One anecdote. Impact: frame check at intake, align digest after design, both skipped when empty.
- **Classical practice.** Keep non-goals, alternatives considered, a one-line scope budget, a one-shot pre-mortem (thin evidence), reference-class rates from own data, door routing enforced by tooling. Drop RFCs, Cynefin, fixed cycles.
- **Decision record.** Falessi: trim to items needed half the time. 63% of 5,800 ADRs opened already accepted. Saebo, Voroshilov: constraints erode; stated purpose and pre-action check help. Impact: `decision`, `why`, `provenance` (user-stated, user-confirmed, agent-assumed), `binds`, `rejected`, `revisit_if`, `check`; injected into bound tasks.
- **Approval.** 74-study review: wrong advice raised incorrect decisions 26%. Explanations raise acceptance regardless. 93% of Claude Code permission prompts accepted. Impact: one screen: agent-assumed decisions ranked by what would change, one-way doors, pre-mortem risks, reference-class mix, review edits that altered a decision, what stops the run. Counts last. Studies are not of experts approving plans.
- **Interrupt on tripwires.** Uncovered one-way door, contradiction of a decision or outcome, failed premise, budget breach; park the branch, continue disjoint work; the rest goes to the milestone digest. Harvest dismissals need a mechanical basis.
- **Fresh context justified by approval bias, not recall.** [Self-Attribution](https://arxiv.org/html/2603.04582): AUROC 0.99 to 0.89/0.92, 5x approval in one setting. Cross-Context Review: no durable F1 gain. Contextual bias: framing moved Haiku 3.5 from 68.4 to 8.5, Opus 4.5 by 6.9. Impact: keep blind finders and verdict-first; contrarian is advisory.
- **794 of 810 measures the merge path.** `review-plan.md` recommended path merges all selected severities, vet drops precede the store. Today 1,011 findings across 44 stores: 995 merged, 16 open, 0 discarded; smallest store 11, median 20, never a clean plan. Code ledgers reject about 5%. Models over-reject. One merge grew a plan 48%. Impact: auto-apply only with mechanical evidence; cited code reading is applied and disclosed; else advisory; anything touching a decision goes to the user.

### plan-state-and-orchestration

Searched: format study, Beads, Kiro, Spec Kit, Taskmaster, Temporal, LangGraph, Inngest, plan drift, event sourcing; newest 2026-11-01.

- **Hybrid representation, on integrity grounds.** 2602.05447 is read-only SQL generation with single runs; stop citing it for or against. Prose for intent, records for anything with id, status, edge or count.
- **Frozen plan cannot be the working plan.** 433 `deviation` entries on 484 tasks (0.89 per task); sample of 28: 23 local, 5 structural. All prose, so unfoldable. `tasks remove` leaves no tombstone. Plan tasks.toml is git-tracked, so `git diff <approved>..HEAD` gives baseline comparison. Impact: freeze `brief.md` and success criteria, stamp `approved_commit`, verbs append `{field, old, new, actor, reason}` with tombstones. Not fold-based.
- **Render-only supported; add a route back.** `plan amend` writes the store with an audit row; `tasks check` fails on dead `design/...#anchor` citations; render to stdout or an uneditable path.
- **Pull-only notes mostly go unread.** Fan: 36 of 64 settings never call `recall_event`, mean -0.36 (elided history, not research notes). Used links: `task_ref` on deviations. Ceremony: Lumina supersession, option branches, epoch stamps; `supersedes_entry` 34/1,330. Impact: inline decisive lines into projections; keep task to brief/design anchor and deviation to task; three projections (worker, orchestrator digest, human render); no field-catalogue copy yet.
- **Orchestration split.** Conversation owns dialogue and gates; one Workflow per stage returns a digest and pointers. Temporal, LangGraph, Inngest suspend in-run; Workflows cannot, and `AskUserQuestion` is unavailable in subagents. Record `{stage, workflow, args_hash, run_id}` at launch; `plan_rev` in args and prompts to bust stale journal hits (inferred, not run); scripts return null agents; agents raise durable questions and return `needs_human`. Correction: whether a Workflow survives its launching session is settled by the docs, as Risks records.
- **Orchestrator context.** 36 sessions, 17.80M chars: skill and command bodies 28.0%, agent returns 12.8%, dispatch prompts 11.4%, Bash 15.3%, Read 7.5%, Write 7.0%. Only one compaction summary, so session end is the likelier loss. Impact: thin carrier, saved Workflows by name, capped digests.
- **Derive stage, do not store it.** 50 of 54 flows `review`, 2 `complete`; 15 fully done stores all still `review`; 47 of 54 idle 30 days. Line 175 targets drafts (1) not unclosed plans (50), and conflicts with line 159. Impact: stage computed from artefacts; drafts excluded from auto-pick, listed with age in the chooser; auto-close when rows done and verification passed. Lumina's gated stages deadlocked.
- **Staleness measures age, not code.** `compute_staleness` hardcodes 7 days. Nothing records a plan's base. Impact: `approved_commit`, warning-first `tasks drift` (diff since approval minus own commits, intersect `files`), replayed over the 15 stores before any blocking.

### Directed research additions

Lens `structural-judgement`, 2026-11-05. Vet: 5 sampled, 0 dropped, 1 downgraded (the HiL-Bench quote is not verbatim; recall and precision confirmed). SlopCodeBench's "cannot stop" needs the qualifier that prompts cut initial erosion by up to 62.3% without stopping iterative degradation, measured on GPT models. Its 53% human figure and SmellBench's kappa came from fetch summaries only.

Searched: agent refactoring and erosion, code sycophancy, LLM debiasing, design fixation, hotspots, fitness functions, escalation benchmarks, dependency hallucination, agent PR rejection, Opus 5 evaluations; newest 2026-10. No evidence found on: a ground-truth test of extend versus restructure, status-quo bias on code, long-horizon quality for Opus 5 or 5.5, fitness functions with agents, named categories versus judgement, expert veto formats, under-scoped PR rates.

- **The bias is a detection gap.** [CodeTaste](https://arxiv.org/html/2603.04177v2): GPT-5.2 69.6% with a detailed spec, 7.9% with a focus area; plan mode 15.1%, average gain 2.2pp, patches 2-3x larger. This is an alignment score over refactoring tasks, so noticing a need during other work is inferred from it, not measured by it. [SlopCodeBench](https://arxiv.org/html/2603.24755): erosion rises in 77% of agent trajectories. Unmeasured for Opus 5 and 5.5. Impact: an unconditional step, usually empty.
- **No general status-quo preference.** Malberg: LLMs favour change in generic vignettes (2024 models). Custom-build share rising 11% to 21.4% by model. Opus 5 can expand scope. [SmellBench](https://arxiv.org/html/2605.07001): the most aggressive resolver added 140 smells. Impact: a missing check, not a disposition to retune.
- **Signals are input, not triggers.** SmellBench: 41 of 65 architecture smells false (63.1%) on one detector and project. Sjøberg: 12 smells unrelated to effort after adjusting for size. Local signals (file count, `cross-cut`) are better grounded. Impact: surface only "this module will force X", with code read.
- **Alternatives help when independently generated.** Same-thread reflection did not debias; self-refine no gain; repeated same-context review lowest. Adversarial rewrite first, cross-model second, with small gains. Merging lost quality. Impact: one fresh target-structure draft; choose or veto; no merge. Adjacent evidence, so replay first.
- **Escalate on decision relevance, not uncertainty.** [HiL-Bench](https://arxiv.org/html/2604.09408): Opus 4.6 34.6% recall, 26.3% precision on software tasks, counting the agent's own `ask_human()` calls against registered blockers (SQL reads 61.2 and 54.3). Calibration Is Not Control: cutting calibration error left regret near 0.318. Impact: three tests; empty is normal.
- **Surface before elaboration.** He et al. (248 lay users): plan editing did not reliably help. AssumptionMiner: mean 3.8 assumptions per task, F1 0.662, so rank and cut. Inference only on Lumina, `plan-story/SKILL.md`: align is MANDATORY in full and light whether or not anything is contestable, frame runs before Explore, align after Decompose, and the tier rule keys on risk signals. Impact: zero to three items after exploration and before decomposition.
- **Mechanise dependency facts.** [2605.17062](https://arxiv.org/abs/2605.17062): 4.62-6.10% hallucination across 199,845 prompts, 127 shared names. Rust crates 20.23%. Agents add a dependency in 1.3% of PRs. Fable 5 hand-rolled caching 57%. Impact: tool pass, then a card showing need and options.
- **Scope errs both ways.** ProMax: Sonnet 4.6 reached about half of gold's files. Opus 5 can widen. Impact: mechanical call-site sweep; surface only materially different readings.
