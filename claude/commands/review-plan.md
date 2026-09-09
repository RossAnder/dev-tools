---
description: Review an implementation plan for feasibility, completeness, risks, and agent-executability
argument-hint: [path to plan file or directory]
---

# /review-plan — plan review across four lenses

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

Reviews an implementation plan document against the actual codebase: validates the plan's assumptions, scope completeness, task executability, and dependency ordering. Works with any plan format — structured work packages, wave outlines, task lists, or prose; agents adapt to whatever they encounter. Findings persist to the flow's `plan-review-findings.toml` keyed by stable `P{n}` IDs; re-runs increment `round` and dedup against prior open items.

> **Agent count**: `/review-plan` uses 4 lens-agents (Feasibility, Completeness, Executability, Risk) — distinct from `/review`'s 5 code-review lenses, because plan review and code review answer different questions.

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and tool usage.

## Step 0: Pre-flight (flow resolution + doctor)

Invoke the `flow-contract-flow-context` skill to load the flow-bootstrap envelope contract (input/output shapes, `envelope.ok` gating, `envelope.resolved.*` and `envelope.doctor.*` binding rules, no-flow fallback, doctor-fail handling, staleness reconciliation, status vocabulary, slug derivation, canonical artifacts, and the mandatory bootstrap-summary console line).

Build the input envelope and dispatch `flow-bootstrap`:

```bash
tomlctl flow envelope build \
  --command review-plan \
  --branch "$(git branch --show-current)" \
  --worktree "$(git rev-parse --show-toplevel)" \
  --cwd "$(pwd)" \
  --staleness-threshold 7d
```

The block above is complete and copy-pasteable as-is — do NOT look up `--help`; `/review-plan` needs no `--require-artifact` flag (it lazily creates its findings artifact), and `--staleness-threshold 7d` is the default, passed explicitly for clarity. On detached HEAD, omit `--branch` so the envelope records `branch:null`. Pass `--flow-override <slug>` when the user supplied `--flow`, and `--path-arg <p>` once per `$ARGUMENTS` path token. Dispatch via the Task tool with `subagent_type: "flow-bootstrap"` and the printed JSON as the prompt. Gate on `envelope.ok`; bind `slug`, `context_path`, `artifacts.*` (incl. `plan_review_findings`), and `doctor.ok` for downstream phases. Emit the bootstrap-summary line before any other action.

## Step 0.5: First-use `plansDirectory` prompt (per-carrier)

Invoke the `flow-contract-plansdirectory-prompt` skill to load the first-use prompt contract (gate on `envelope.plans_directory == null`, option-list construction, single-select AUQ ordering, headless empty-answer in-memory binding, `Don't ask again` sentinel arbitration, free-text follow-up, persist-via-`tomlctl json set`, and downstream binding). The wording is shared verbatim across `/plan-new`, `/plan-update`, and `/review-plan`.

## Step 0.6: Ensure the task store

Invoke the `flow-contract-task-store` skill to load the store contract (the schema and every row field, the `ref` rule, the status vocabulary, the semantics of each `tasks` verb, the `check` finding classes and exit policy, the ref-set diff gate and the renamed-heading trap it catches, and the orchestrator-only status-write rule).

`.claude/flows/<slug>/tasks.toml` holds the plan's task DAG, execution policy and checkpoint groups, and this command reads two things from it: the invariant checks Agent 1 would otherwise re-derive by hand (Step 2), and the ref-set gate over the merge's own rewrite (Step 4). Bind `<store>` as `envelope.resolved.artifacts.tasks` (fallback `.claude/flows/<slug>/tasks.toml`); every `tasks` call below targets it with `--slug <slug>`. Import before anything reads a row status — on a first run because the store is absent on disk or holds no rows, and on **every later run** because the flow's execution record can hold completions the store has not seen. Re-running is idempotent: the upsert is keyed on `ref` and `--reconcile-record` only ever promotes a row to `done`, never demoting one. Step 4's ref-set gate turns on row status, and a row left `pending` under a completion the record already holds reads as a task the merge may safely orphan.

```bash
tomlctl tasks import-plan --slug <slug> --reconcile-record
```

**Plan resolution is pulled forward into this step.** The third skip case below compares the plan under review against the flow's `plan_path`, so the comparison has to be settled before `--slug` writes anything — run Step 1's resolution order here, bind `<plan>` from it, and let Step 1 read the documents `<plan>` names rather than re-resolving them. Step numbering is unchanged — only the resolution moves.

**Three cases skip the store wholesale.** Take them before the import, and name the one that fired in the Step-3 report — a reader who sees no `STORE CHECK` fence must be able to tell a skip from a clean bill: a **no-flow run** (Step 0's fallback resolved no slug, so nothing owns a store); a **`<parent>-tdd-<NNN>` sub-flow slug**, exempt per the contract because the parent flow owns the plan and therefore the store; and a **`<plan>` that is not the resolved flow's `plan_path`** (`--slug` would import a different document over every row). Everything else in this command runs unchanged in each case — the store is an input to the review, never a precondition for it.

**An error-class finding is review material here, never a halt.** `/implement` halts on one because it schedules from the store; `/review-plan` exists to read plans that are wrong, and a cycle or an invalid cut is precisely the defect it is being asked to find. A real import refuses to write when it hits one, which leaves no store for `check` to read — so fall back to plan mode and carry that envelope's `findings[]` into Step 2 in place of the `check` output:

```bash
tomlctl tasks import-plan --plan <plan> --dry-run
```

# Plan Review

## Step 1: Load the Plan

**Reason thoroughly through plan analysis** before dispatching agents. This is the resolution order Step 0.6 already ran to bind `<plan>` — read what `<plan>` names rather than resolving a second time: (1) if `$ARGUMENTS` is a file path, read it; (2) if a directory, treat as a **multi-file plan** — read all markdown files, classify each by role (outline/master = primary; numbered detail docs = actionable tasks; progress/status = completion + deviation context; diagrams/supporting = reference), build a document map, share it with all agents; (3) if empty, locate the active plan in order: just-produced-in-conversation → Step-0 resolved flow's `plan_path` (via `tomlctl get <context_path> plan_path`; flag prominently when `envelope.resolved.stale == true`, >14 days) → recently-modified file in the plans directory (ask if multiple) → ask the user. Read the full content of every in-scope document.

## Step 2: Launch Parallel Review Agents

Invoke the `flow-contract-task-visibility` skill for the run-scoped task-surface contract (view-not-store rule, subject prefix with lowercase `<ref>`, `activeForm`, lifecycle, granularity floor, silent degradation). Mint one task per lens, subject-prefixed `<slug> /review-plan · <lens>`, before launching; `TaskUpdate` each `pending → in_progress` on launch and `→ completed` only after its Step-2.5 vet. Never mint a task per finding — the findings ledger owns per-item state.

Launch **all four** review agents in a single response message (concurrent execution mandatory) via the Agent tool with `subagent_type: "research-deep"`. `research-deep` across all four is non-negotiable — plan critique is pure judgement and the fetch-and-summarise contract produces superficial findings that miss real defects; **do NOT reduce the count or downgrade any agent to `research-lite`**. Each agent reads the plan in full, explores the actual codebase to validate the plan's claims (read referenced files, search assumed patterns, verify paths and line numbers), and returns **up to 20 findings, target 15 — a ceiling and a target, never a quota** — with references to specific plan sections; the same budget `/review` states, so the two carriers do not diverge. Returning zero is a correct result when a lens finds nothing; say in one line what you examined and ruled out. Thoroughness is coverage, not count — a padded finding costs more than a missed one, because it is triaged, merged, and re-raised every round. State the budget in every lens prompt: the agent's own default is lower, and an unstated budget is one the agent resolves for itself.

**Enumerate, don't sample.** A finding that covers N instances of one defect MUST name all N in its `description` (`"three Depends-on edges absent from the graph block: 1→4, 6→9, 6→10"`). Merging instances into one finding is correct and keeps the `(plan_section, anchor_old)` dedup key intact; *sampling* — reporting two instances and moving on — is the failure mode that lets a defect class survive review with findings already filed against it. When a lens cannot bound N, say so explicitly (`"≥4 sites; enumeration incomplete"`) rather than implying the list is closed.

**Format-contract embedding (orchestrator, before launch):** when the plan follows the house format (`## Tasks` / `## Dependency Graph` present), invoke the `flow-contract-plan-output-format` skill in the main conversation and embed its **Format rules** block plus the `## Execution Policy` template semantics verbatim into Agent 3's prompt inside a clearly-delimited `FORMAT CONTRACT` fence. `research-deep` agents cannot invoke skills, and a repo-relative path to the skill file resolves only inside the harness's own repo — the orchestrator carries the contract to the agent, which keeps `/review-plan` portable to any project. For plans not in the house format, omit the fence; Agent 3 critiques structure on general executability judgement only and never penalises a foreign format for missing house sections.

**Store-check embedding (orchestrator, before launch):** run the store's own invariant checks and embed the JSON verbatim in **Agent 1's** prompt inside a clearly-delimited `STORE CHECK` fence:

```bash
tomlctl tasks check --slug <slug> --plan
```

Exit `1` means an error-class finding — a cycle, a dangling edge, a duplicate task number, an invalid checkpoint cut — which is a **critical** review finding, not a reason to stop (Step 0.6). `--plan` adds `render/drift`, a warning meaning the markdown carries wording the store's render does not reproduce; report it and never render over it, because outside the Step-4 merge this command does not rewrite the plan. When Step 0.6 skipped the store, omit the fence **and say so in the prompt** — an absent fence is otherwise indistinguishable from a clean one.

The four lenses:
- **Agent 1 — Feasibility, Dependencies & Execution Policy**: are changes feasible given current architecture, and APIs/versions current? Are task/phase dependencies correct, with no hidden ordering hazards — and no two tasks whose `Files` intersect while lacking a dependency path between them? (`/implement` frontier-schedules from the DAG edges: "parallel" means *no dependency path*, not "different batch" — an edge-less same-file task pair is a defect even when prose waves appear to separate them; /implement's dispatch-time file-claim check serialises such pairs defensively, but the missing edge is still a plan defect to flag.) When the plan declares an `## Execution Policy`, judge broken-state hazards against **checkpoint boundaries**, not dependency levels — transient intra-group breakage between checkpoints is by design; each checkpoint group must be a logically-coherent, buildable increment. Validate the policy structurally: checkpoint markers reference real task numbers and form valid topological cuts (no task in group k depends on a task in a later group). Absent the section, fall back to per-dependency-level broken-state judgement (legacy per-batch semantics). Also check **checkpoint coverage**: walk each marker's dependency closure and diff against the task list — a task inside no marker's closure is committed only by the final Phase-3 train, forfeiting the bisectability that chose `milestones` over `single`. Report the orphan set in one finding, enumerated. The `STORE CHECK` fence in your prompt already carries the computed answer to the mechanical half of all three questions, over the edge set **as written** — `dag/unreachable-claim` for an edge-less same-file pair, `checkpoint/invalid-cut` for a prefix union that is not downward-closed, `checkpoint/orphan-task` for the coverage walk, which reports a row carrying a group id no `[[checkpoints]]` entry declares alongside one carrying no group at all — so read those as ground truth for the edges the plan states rather than re-deriving them. **Whether the edge set is complete is still the lens's job**: `check` validates the DAG it is handed and cannot see a task consuming what another task builds with no edge between them, so a clean fence is evidence about the edges present, never that none is missing. Spend the lens there and on the rest of what a graph cannot answer: whether a present edge is the *right* edge, plus the feasibility and coherence judgements above. No fence means the store was skipped, not that the graph is clean — do the walks by hand and say which.
- **Agent 2 — Completeness, Scope & Codebase Alignment**: affected-but-unmentioned files/components/consumers, missing tests, config/migration/build changes, cross-cutting concerns (logging, error handling, authz, cache invalidation), and same-pattern code elsewhere needing the same treatment. Also owns **alignment**: do referenced files/classes/methods/paths exist and look as the plan assumes, and do cited line numbers still point at the cited symbol (summarise drift in one enumerated finding — line drift is low-severity; a *non-existent filename* is not, because nothing downstream re-derives it). Run this as an explicit procedure, not an impression — the completeness lens is the one that historically returns *under* its budget while missing the most:
  1. For every symbol the plan renames or deletes, grep the whole tree — including **comments and docblocks in source files**, `.md`, and **user-visible string literals** — not only doc directories and not only with-paren spellings. A code change silently falsifies prose, and a copied literal of a deleted message keeps passing because it only ever fed a mock.
  2. For every new registration seam (provider, plugin, registry, hook), find the production entry point that would call it. No call site anywhere in the plan ⇒ the whole tier is dead code that passes every test.
  3. For every consumer a task removes, grep for anything citing that consumer as *justification* (a barrel's publication bar, an allowlist's membership rationale). A documented inclusion criterion is a dependency.
  4. For every enumeration, allowlist, or count the plan transcribes, re-derive it and compare. Report the delta with the command you used.
  5. For every premise the plan asserts about current *behaviour* rather than current structure — "X already transitions", "Y is read by Z", "this path is unreachable today" — find the code that settles it or run the command that measures it. The alignment check above reaches existence only, and a symbol can exist while nothing reads it; these premises are what make a task's **Action** unnecessary or its **Detail** wrong, and they read as authoritative precisely because nobody states an assumption they think is uncertain. Report each as plan-assumes-vs-measured.

  **Return one line per sweep even when it finds nothing** (`sweep 2 (seam call sites): 1 seam, call site present at server/index.ts — OK`). A sweep that produced no finding and a sweep that was never run are indistinguishable otherwise, and this lens historically returns *under* budget while missing the most.
- **Agent 3 — Agent-Executability & Clarity**: clear imperative actions, exact files named, **per-task Files↔body parity** (flag any edit target named in a task's Action/Detail/Acceptance that is absent from its **Files** line — Acceptance-named test files are the classic omission; `/implement`'s file-claim scheduling, lite-eligibility gate, and failure rollback all trust the Files line verbatim, so an omission is a scheduling defect, not cosmetics), verifiable acceptance criteria, right-sized tasks, no executor-time architectural decisions, parallelisable with no file overlap. Judge structure against the `FORMAT CONTRACT` fence embedded in your prompt (see the format-contract embedding step above) as the single source for sizing, per-task file caps, and parallelism conventions (including the `## Execution Policy` fields — max-parallel within ceiling, valid commit granularity) — never against assumed conventions or training priors; when no fence is present, the plan is foreign-format: critique on general executability only. Suggest restructuring if prose-format.
- **Agent 4 — Risk & External Validity**: use Context7 to verify API signatures/parameters/options against versions in use; WebSearch for deprecations/advisories/breaking changes; known pitfalls, realistic scope/effort, rollback adequacy **relative to the declared checkpoint policy** (the cadence itself is a user decision — review checkpoint *placement*, not existence: risky tasks such as migrations and public-API/schema changes should be immediately followed by a checkpoint; flag a `single`-policy plan carrying a mid-flow risky task; do NOT recommend re-adding per-batch commits merely because the plan chose a coarser cadence), and unaddressed performance/security/back-compat risks.

## Step 2.5: Vet agent output (orchestrator)

Invoke the `flow-contract-vet-research` skill to load the universal vet-pass procedure (triage by source+evidence-grade, `ESCALATE-TO-DEEP` honouring, drop-low-confidence rule, spot-check sampling, drop/downgrade-with-rationale, the canonical `[[vet_events]]` append heredoc, the mandatory `vet: Agent-{n} (<lens>) — N sampled, M dropped, K downgraded` console line, and the >30% systemic-failure re-dispatch rule).

**Per-carrier sample size**: spot-check ≥ 3 findings per agent (or all if fewer). Lens names: `feasibility`, `completeness`, `executability`, `risk`. **Verify every "stale reference" / "file does not exist" / "API has changed" claim BEFORE sampling** (file-does-not-exist → `ls`/Glob; API-changed → re-query Context7; signature mismatch → Read at cited line); drop any claim whose verification fails, with the verification evidence as the `[[vet_events]]` `rationale`. This verification is the highest-value step for plan critique — a reviewer that flags non-existent stale references is worse than no reviewer.

## Step 2.6: Orchestrator-only checks (run before consolidation)

Four defect classes the lens agents are the wrong place for. `research-deep` does hold Bash and may verify tooling-behaviour claims within its own lens — but the baseline half of check 1 and the whole of check 4 are **whole-tree** commands whose results every lens's findings depend on, and four lenses each running them in parallel against one shared build directory buys nothing and serialises on the build lock. Run them once, here. Checks 2 and 3, and check 1's precondition half, are **whole-plan cross-task scans**, and the orchestrator is the only participant holding the full document plus every lens's findings at once. A lens partitioned to one perspective cannot see a contradiction that spans two tasks.

1. **Baseline and precondition-check the predicted break sets.** Any **Acceptance** that predicts what a compiler, type-checker, or linter will report ("errors only in these four files", "this goes red until task N") is a claim about tooling behaviour, and such claims are wrong often enough to be worth the seconds. Run each break-set command against the current tree to capture the **baseline** — a command already red pollutes every prediction downstream of it — and to record which files the named config actually sees. Then check the prediction's preconditions statically: is each named file inside the config's `include`/`exclude`; is the lint rule that would fire actually enabled; does the construct the prediction relies on even produce a diagnostic. **Do NOT diff observed against predicted** — a break set describes the tree *after* the task lands, which does not exist yet and which this command must never create; that comparison would fire on every forward-looking prediction in every correct plan. A failed precondition is a **critical** finding: an over-predicted break set silently removes a later task's only signal, and an under-predicted one sends an agent chasing another task's breakage. Guards that silently do not fire are the recurring shape (`noUnusedLocals` off; a doc-comment reference is not a use; an optional key is never *required*, so adding one produces zero errors).
2. **Cross-task acceptance contradiction.** Scan every acceptance for the state it presumes, then check no *other* task removes that state. One task asserting against rows a sibling deletes cannot be satisfied by any execution order — it is unresolvable at runtime and costs a repair task. Report as **critical**, naming both tasks.
3. **Falsifiability.** For each acceptance, name the input that makes it fail. Flag any that can pass vacuously: both sides of a comparison `undefined`, an optional key the type system never requires, an empty match set, a path filter matching nothing and exiting 0. An acceptance that cannot fail is not an acceptance, and the plan-under-review is often *alert to this class in the abstract* while endorsing instances of it.
4. **Probe every runnable acceptance command (two-control sweep).** Check 3 is static reasoning, and static reasoning is exactly the mode that endorses a broken command — the defects here are *tool semantics*, which read as correct on the page. So run them: every read-only shell criterion gets a negative control against the current tree and a positive control against the input a correct implementation would produce, per the plan-output-format skill's **two-control rule**, using its **Acceptance probe helper** (invoke the skill here if Step 2 did not) as one batched shell call in which every probe reports. **Classify each criterion's polarity first** — forward (describes the post-change tree) or falsifier (describes the pre-change tree: "the suite fails on the old values", "goes red until task N") — because the verdicts invert and a falsifier read with forward polarity is certified healthy while being permanently green. Treat any "test X passes" acceptance as carrying an implicit falsifier and probe that. Severity by verdict: **unsatisfiable** (both controls fail) — critical, with both observed values and the corrected command, since no correct implementation can pass the task; **permanently green** (a falsifier that does not hold today — the named check does not bind what the task changes, the shape a suite asserting only ordering takes when both the old and the new values satisfy it) — critical, naming the assertion that would bind; **broken** (`No such file` / `command not found` / `fatal` in the stderr column) — critical; **vacuous** (a forward criterion that already holds) — critical unless the line is tagged `(regression guard)` with a discriminating criterion beside it; **healthy** — note the baseline so the plan can record it.

Findings from this step enter Step 3 alongside the lens findings, categorised `feasibility` (1) / `executability` (2, 3, 4).

## Step 3: Consolidate Results

**Reason thoroughly through consolidation.** Cross-reference all surviving (post-vet) findings against the plan, resolve conflicting assessments, deduplicate across agents, and synthesise a single consolidated report. For every critical issue, include what the agent found in the codebase that contradicts the plan. An empty review is valid — a well-written plan may have no issues. The report header is `## Plan Review: [plan name/path]` followed by **Plan scope**, **Plan age** (flag if >14 days), and **Overall assessment** (Ready to execute | Needs revision | Major gaps), then severity-grouped sections (Critical / Warnings / Suggestions, each entry `[plan section/task] (area) Description`), a **Stale References** section (plan-assumes-vs-codebase-shows per item, or "All references verified current."), and an **Executability Assessment** (file coverage / dependency graph / parallel safety / execution policy / acceptance criteria / stale references).

## Step 3.5: Persist Findings

After Step 3 and before Step 4, persist findings to the flow's `plan-review-findings.toml` so subsequent runs dedup and Step 4 has a single source of truth.

**Before the first TOML mutation, invoke the `tomlctl` skill** to load the full CLI surface (`items next-id` / `add-many` / `array-append` / `items apply` / `set` / readback). Drive every read and write of `plan-review-findings.toml` through `tomlctl` — never line-edit the TOML, and do **not** probe `tomlctl --help` (the skill is authoritative for subcommands and flag spelling; `--help` round-trips waste a turn and invite invented flags such as `--format json`).

1. Resolve `plan_review_findings_path` from `envelope.resolved.artifacts.plan_review_findings`; for legacy flows derive `.claude/flows/<slug>/plan-review-findings.toml` per the `flow-contract-flow-context` self-healing contract and write it back to `[artifacts]` on the next TOML write.
2. No manual bootstrap if the file does not exist — the first `tomlctl items add-many` / `set` write (steps 4-5 below) auto-creates and seeds it with the schema-aware skeleton (`schema_version = 1` + `last_updated`, byte-identical to `flow init`), reporting `"created": true` in its envelope. (No atomic dance either way — `/review-plan` is the sole writer.)
3. Mint monotonic IDs via `tomlctl items next-id <path> --prefix P`.
4. Batch-write: `tomlctl items add-many <path> --defaults-json '{"review_round":<n>, "status":"open"}' --ndjson <path or ->` — a staged NDJSON file for a batch of more than a few rows, `-` with the single-line `printf` pipe otherwise (see the tomlctl skill).
5. `tomlctl set <path> last_updated <today>` and `tomlctl set <path> round <n>`, where `<n>` is the current review round (1 on first run; increment per Re-run dedup).

### Artifact Schema: `plan-review-findings.toml`

Required fields: `id` (`P{n}` monotonic), `review_round` (int), `severity` ∈ {`critical`, `warning`, `suggestion`}, `category` ∈ {`feasibility`, `completeness`, `executability`, `risk`}, `plan_section` (markdown heading anchor, copied verbatim from the plan), `summary` (one line), `status` ∈ {`open`, `merged`, `discarded`}. Optional: `description`, `anchor_old` (exact substring already in the plan under `plan_section`), `anchor_new` (replacement). **The `anchor_old` + `anchor_new` pair is the mechanical merge contract — BOTH required for auto-merge; anchor-less findings are advisory-only and skipped by the merger.** Schema callouts: `tomlctl items find-duplicates` / `orphans` hardcode the review/optimise schema and MUST NOT run against this file; `next-id --prefix P`, `items list`, `add-many --ndjson -`, and `apply --ops -` are the supported subcommands.

## Step 4: Merge Offer (end of turn)

Let the user merge selected-severity findings into the plan. **Two modes**: a **Manual merge + accept** fast path (one judgement-based rewrite over the original, then end — skips the mechanical-merge → accept → manual-fixup cycle) and the **Mechanical merge** path (anchor-based, conflict-safe, produces a `.revised.md` sibling for review).

1. **Count findings by severity.** If zero total, output `No findings — plan is clean.` and end.
2. **`AskUserQuestion` — ask both questions in one call:**
   - **Q1 "Severities"** — `multiSelect` over `[Critical, Warning, Suggestion]`, default `[Critical, Warning]`.
   - **Q2 "Merge mode"** — single-select:
     - **Manual merge + accept** *(recommended)* — judgement-merge **all** selected-severity findings (anchored *and* advisory; same-section clusters resolved by reasoning) directly into the plan, write over the original, transition them to `merged`, and end. One rewrite, no second prompt. → Step 4A.
     - **Mechanical merge → review** — anchor-based, conflict-safe; produce a `.revised.md` sibling, then a follow-up Accept / Keep both / Discard prompt. → Step 4B.
   - **Empty-answer rule**: if the response comes back empty (running in `acceptEdits` / skill-hosted / headless mode, per Claude Code issues [#29618](https://github.com/anthropics/claude-code/issues/29618) and [#29547](https://github.com/anthropics/claude-code/issues/29547)), treat as **zero severities selected** → **SKIP merging, persist findings only, end.** Never silently run **Manual merge + accept** — it overwrites the original, so it requires an explicit interactive selection.
3. **If zero severities selected** → persist only, no merge (the Q2 merge mode is moot — nothing is merged regardless of mode). Output: `Findings persisted; merge skipped. Re-run interactively to merge.`

### Step 4A: Manual merge + accept (fast path)

A1. **Select findings** — every `open` finding whose severity is in the Q1 set, both anchored (`anchor_old`/`anchor_new`) and advisory (anchor-less). Manual mode reasons over all of them; it is **not** limited to the mechanical anchor contract.
A2. **Merge with judgement** — apply each finding to the plan: use `anchor_new` as the intended wording where present, the finding `description` as intent where not. Resolve same-`plan_section` clusters (the case the mechanical path conflict-skips) into one coherent edit. Preserve the plan's structure and section order; change only what the findings require. **Files-line consistency**: when a merged finding adds or changes edit requirements in a task body, update that task's **Files** line in the same edit — the Files-line closure rule from the `flow-contract-plan-output-format` contract must still hold after the merge.
A3. **Back up, then write over the original** — first copy the current plan to a `<plan>.premerge.md` sibling (cheap rollback for this no-checkpoint fast path), then `Write` the merged content over the original. This *is* the accept; no `.revised.md` is produced (the `.premerge.md` sibling and git history are the recovery paths).

**Retention — one generation, and it is terminal.** If a `<plan>.premerge.md` already exists, **delete it** before writing the new one; never rename it to a `.prev` sibling, because the prior generation's plan text is already in git. The sibling then survives exactly until the flow closes: `/plan-update <slug> complete` deletes it unconditionally (step 6.5 there), as it does `.revised.md` and `.revised.prev.md`. Without a terminal owner a "one-cycle" rule never fires on the *last* run of a flow, which is how a repo accumulates dozens of immortal siblings — each a near-duplicate of a plan that is itself in git. Do not commit `.premerge.md`; add `*.premerge.md` to `.gitignore` where plans are tracked.

A3.5. **Merge-exit consistency re-derivation (mandatory, against the written file).** A merge that edits **Files** lines, **Depends on** lines, or task numbering can *create* cross-task defects that did not exist in the reviewed plan — the merge is the least-reviewed text in the document, and nothing downstream re-reads it. This runs *after* A3 precisely so there is a file on disk to import and check; `.premerge.md` is the rollback if the re-derivation goes wrong.

  **(a) Ref-set gate — first, before any real import.** A merge may rephrase a heading, and the heading's title is what derives the row's `ref`, the store's primary key and the import's upsert key. Read the dry run's `added_refs` / `removed_refs`:

  ```bash
  tomlctl tasks import-plan --slug <slug> --dry-run
  ```

  Abort for user intervention on any removed ref whose row is not `pending` — a settled task about to be orphaned — and surface the added/removed pair either way:

  ```bash
  tomlctl tasks list --slug <slug> --where-in ref=<removed-refs> --select id,ref,status
  ```

  Recovery for an intentional rename is to rename the row, not the store, then re-run the gate:

  ```bash
  tomlctl tasks update <id> --slug <slug> --ref <new-ref>
  ```

  **Never let the real import be what discovers a rename.** The old row is never deleted, so it still holds the task number the renamed heading re-claims: the import raises `dag/duplicate-number` at error severity and refuses — after the merge has already rewritten the plan. Left unresolved the other way, the orphaned `done` row and its duplicate `pending` twin make `/implement`'s idempotency skip-list re-execute a completed task.

  **(b) Real import, then check.** Once the diff is clean or resolved, upsert and re-derive:

  ```bash
  tomlctl tasks import-plan --slug <slug>
  tomlctl tasks check --slug <slug>
  ```

  `dag/unreachable-claim` is the file-claim reachability answer — a file on ≥2 rows' `files` with no directed path *between the two claimants*, computed pairwise, a shared **ancestor** not counting as a path. `checkpoint/invalid-cut` and `checkpoint/orphan-task` are the checkpoint-marker answer, the second naming a task a renumbering dropped outside every marker's closure, or one left pointing at a group id no `[[checkpoints]]` entry declares. Five more classes the pair can return are **error-class** and so refuse the real import above rather than reporting through it: `dag/unbuildable`, `plan/no-tasks` for a `## Tasks` section the grammar reads as holding no task at all, and `policy/checkpoints-value` / `policy/commit-granularity-value` / `policy/origin-value` for a `[policy]` field outside its vocabulary. Each is critical — a plan the store cannot hold is not a plan `/implement` can schedule. `plan/effort-untagged` adds a single warning naming every heading left without an `[S|M|L]` tag. Fix what they name in the plan with `Edit`, then re-run both. Report the row count they were computed over, not only the violations — an unreported denominator is how a sampled check passes for an exhaustive one.

  **(c) Files-line closure — prose, per A2**, for every task body the merge touched: every edit target named in **Action**/**Detail**/**Acceptance** appears on the **Files** line. `tasks check` ships a `files/closure` class, but it is **info**-severity and moves no exit code: it lists the backticked paths under `packages/`, `apps/`, `docs/` or `scripts/` a row names outside its `files`, skipping lines whose prose marks the reference read-only. Read it as a candidate list, not an answer — as a gate the same check flagged 80 of 117 path tokens across 28 tasks, because the format requires a read-only reference to stay off the `Files` line. Separating an edit target from a reference is this step's judgement, and nothing mechanical makes it.

  Do NOT mirror `Depends on` edges into `## Dependency Graph`: that section carries checkpoint markers only, and the per-task edges are authoritative. A merge that renumbers tasks must fix the markers, not re-transcribe the graph. When Step 0.6 skipped the store, (a) and (b) have nothing to run against — walk the multi-claimed files pairwise and each marker's closure by hand instead, and say in A6 that they were hand-derived.

  Record what the re-derivation changed; it is reported in A6. Never emit a self-audit block (a "File-claim check" paragraph or similar) asserting the plan is clean — a transcribed assertion goes stale the moment a later edit lands, and the next reader inherits false confidence from it. State the edges; let the reader re-derive.
A4. **Transition** every merged finding to `status = "merged"` in one batch via `tomlctl items apply <path> --ops -`.
A5. `tomlctl set <path> last_updated <today>`.
A6. **Console summary**: `N findings merged into <plan>`; list `plan_section → summary` per finding, then one line per A3.5 re-derivation fix (`[merge-exit: <task> — added edge N→M; pipelineApi.ts claimed by both]`), the store line carrying the denominator (`store: 37 rows imported (2 added, 1 updated); tasks check clean`), and the ref-set diff whenever either side was non-empty (`refs: +wire-the-render-verb / -wire-the-renderer (id 21, pending)`). When the merge changed any **Files** line, **Depends on** line, or task count, close with `merged text is unreviewed — re-run /review-plan for round 2`. End the turn — no further prompt.

### Step 4B: Mechanical merge → review

B1. **Filter selected-severity findings** to those with **both `anchor_old` AND `anchor_new`**; advisory-only findings are skipped silently.
B2. **Conflict detection** — group filtered findings by `plan_section`; if >1 in a group has non-empty `anchor_old`, emit `[conflict: plan_section="..."; findings=P3, P7] — manual merge required` and skip that whole group (other groups still apply).
B3. **Mechanical merge** — for each survivor, locate `anchor_old` as a substring under its `plan_section` heading; if found exactly once, replace with `anchor_new`, else log `[merge-failed: P{n} — anchor_old not found uniquely in section "..."]` and skip. Apply in P-id monotonic order. **Files-drift detection**: the mechanical path never rewrites a task's **Files** line itself — so when an applied `anchor_new` introduces an edit-target file path into a task body that the task's **Files** line omits, still apply the replacement but record the pair for a `[files-drift: P{n} — <path> required by merged body text but absent from **Files**]` warning in B6 (the user fixes it by hand, or re-runs with Manual merge which owns Files consistency).
B4. **Materialise** via `Write` to a sibling: replace the plan's trailing `.md` with `.revised.md` (do not append). Then run the A3.5 re-derivation against that written file as a **dry run only** — the revised sibling is a proposal the user has not accepted, and the store must not learn it before B8 does:

```bash
tomlctl tasks import-plan --slug <slug> --plan <plan>.revised.md --dry-run
```

One envelope carries both halves: `added_refs` / `removed_refs` for A3.5(a)'s ref-set gate, and `findings[]` for the classes A3.5(b)'s `check` would report. The mechanical path may not rewrite **Files** lines and may not write the store, so it does not fix what it finds — record each `dag/unreachable-claim`, `checkpoint/invalid-cut`, `checkpoint/orphan-task`, `dag/unbuildable`, `plan/no-tasks`, `policy/checkpoints-value`, `policy/commit-granularity-value`, `policy/origin-value` and `plan/effort-untagged` for a `[merge-exit: …]` line in B6, and each removed ref whose row is not `pending` for a `[ref-orphan: …]` line. `checkpoint/invalid-cut`, `dag/unbuildable`, `plan/no-tasks` and the three `policy/*` are error-class: B8's real import would refuse on any of them, so an accepted revision carrying one leaves the store behind the plan. A3.5(c)'s Files-line closure stays prose here too, and the no-self-audit-block rule holds. For multi-file plans (`plan_path` → `<dir>/00-outline.md`), materialise only `<outline-dir>/00-outline.revised.md` — detail files are not rewritten by v1.
B5. **Pre-existing sibling**: if `<plan>.revised.md` already exists, rename it to `<plan>.revised.prev.md` first (overwriting any older one). Cheap rollback.
B6. **Console summary**: `N applied, K conflicts skipped, M merge-failures`; list `plan_section → summary` per applied finding, then one `[files-drift: …]` line per pair recorded in B3, then one `[merge-exit: <class> — <finding detail>]` line per violation recorded in B4 (`[merge-exit: dag/unreachable-claim — pipelineApi.ts claimed by tasks 4 and 9 with no dependency path between them]`), then one `[ref-orphan: <ref> — id N is <status>, not pending; heading rephrased by the merge]` line per removed ref B4 flagged. Keep the three tags distinct: files-drift is a body/**Files** mismatch inside one task, merge-exit is a store finding the revised text would create, ref-orphan is a settled store row the revised heading would strand.
B7. **`AskUserQuestion` (Q3)** — `[Accept, Keep both, Discard]`. **Default `Keep both`** (NOT `Accept` — `Accept` is irreversible, and default-Accept + auto-mode empty-answer = silent overwrite). **Empty-answer rule**: empty → treat as `Keep both`.
B8. **Apply chosen action**: **Accept** → `Write` revised content over the original, keep `<plan>.revised.md` one cycle, transition matching findings to `status = "merged"` via `tomlctl items apply <path> --ops -`, then upsert the store from the now-accepted plan and re-check it — B4's dry run gated this write, and Accept is the point at which the markdown the store mirrors actually changed:

```bash
tomlctl tasks import-plan --slug <slug>
tomlctl tasks check --slug <slug>
```

Abort the import instead, per A3.5(a), if B4 recorded a `[ref-orphan: …]` the user has not resolved with `tasks update <id> --slug <slug> --ref <new-ref>`. **Keep both** → no mutation; findings stay `open` and the store is untouched, matching a plan file that did not change. **Discard** → delete `<plan>.revised.md`, transition findings to `discarded`; the store is likewise untouched. The prior run's `<plan>.revised.prev.md` is deleted on the NEXT run's B5 (one-cycle retention).
B9. `tomlctl set <path> last_updated <today>`.

### Re-run dedup (subsequent invocations)

Subsequent runs increment `round`: read via `tomlctl get <path> round`, increment, write back via `tomlctl set <path> round <n>`. `discarded` findings are ignored by lens-agents. `merged` findings are passed as **merge-provenance context**: the `plan_section`s they rewrote are the document's least-reviewed text — written by the previous round's merge and read by nobody since — so agents read those sections FIRST and are told the prior finding's intent so they can judge whether the merge actually achieved it without introducing a new defect. `open` items from prior rounds are passed as prior context so agents avoid re-raising. Dedup key `(plan_section, anchor_old)` — a new finding matching an existing `open` item MUST NOT be added; update the existing item if severity/category changed, otherwise skip.
