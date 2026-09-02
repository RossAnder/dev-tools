---
name: flow-contract-apply-pipeline
description: "Canonical end-to-end pipeline contract shared by the apply-flow carriers (/review-apply, /optimise-apply) — the Step 0→6 orchestration each carrier runs: pre-flight envelope gating, ledger location + selector semantics (ID-prefixed vs legacy numeric, empty-ledger clean exit, deferred hard error, terminal-disposition idempotency, mixed-batch partial success, --file-budget / --allow-cross-file overrides), the freshness gate, pre-analysis (Explore delegation with the paraphrase-not-quote injection guard, selector caps, deleted-file detection, Tier-1/Tier-2 already-applied tests, the no-bytes-written disambiguation rule), file clustering, agent dispatch + prompt contract, the interim checkpoint, verification with applied-claim diff reconciliation and the regression cross-check, ledger mutation and the two-call write pattern, the final-summary skeleton, and deviation follow-up. Carriers bind a small vocabulary interface (id prefix, dispositions, producer command, ledger paths) and state only their domain-specific deltas. Consult when running or editing an apply-flow command."
---

# Apply pipeline (shared contract)

Governs `/review-apply` and `/optimise-apply`. Everything below is carrier-agnostic; each
carrier binds the vocabulary interface in § *Carrier vocabulary* and states only its own
domain deltas (narration rules, category-specific verification, critical-gate categories).

## Carrier vocabulary

Each carrier declares these bindings once, at the top of its file. Every `<TOKEN>` in this
contract resolves through that table.

| Token | Meaning |
|---|---|
| `<CMD>` | the apply command itself (`/review-apply`, `/optimise-apply`) |
| `<PRODUCER>` | the command that produced the ledger (`/review`, `/optimise`) |
| `<ID>` | item-ID form (`R{n}`, `O{n}`) |
| `<LEDGER>` | flow artifact key + flow-less fallback path |
| `<APPLIED>` | disposition for "bytes written, finding resolved" (`fixed`, `applied`) |
| `<REJECTED>` | disposition + rationale field for "will not apply" (`wontfix`/`wontfix_rationale`, `wontapply`/`wontapply_rationale`) |
| `<NO-CHANGE>` | disposition + note field for "code already correct, no bytes written" |
| `<TERMINAL>` | dispositions that make re-selection a no-op |
| `<CRITICAL-CATEGORIES>` | categories that gate silent `<REJECTED>` transitions behind user confirmation |

Where a carrier's schema has no distinct no-change disposition, `<NO-CHANGE>` binds to
`<REJECTED>` with a moot-finding rationale — the semantic intent is "the finding is moot",
not "rejected on merit", but the tag is forced by the schema's disposition vocabulary.

## Step 0: Pre-flight

Each carrier builds its own `tomlctl flow envelope build` invocation (the flags differ) and
dispatches `flow-bootstrap` via the `Task` tool with the printed JSON as the prompt; parse
the reply as `envelope`.

1. **Gate on `envelope.ok`.** If `false`, surface `envelope.errors` verbatim and halt — no
   scope analysis, no downstream phase.
2. **Bind**: `slug`, `context_path`, `artifacts.*`, `doctor_ok = envelope.doctor.ok` (when
   `envelope.doctor` is non-null), `envelope.resolved.stale`. Emit the bootstrap-summary
   console line before any other action.
3. **No-flow fallback**: `envelope.resolved.resolved == false` → use the carrier's flow-less
   convention (`<LEDGER>`'s fallback path). `envelope.resolved.tie_candidates`, when
   non-empty, lists the slugs to surface for the user prompt.
4. **Doctor-fail**: `envelope.doctor.ok == false` → surface the failing `envelope.doctor.checks`
   and ask the user **before** mutating any artifact. Auto-repair (`tomlctl flow doctor --fix`)
   is the orchestrator's call; bootstrap is read-only.
5. **Staleness**: when `envelope.resolved.stale.stale` is `true`, invoke the `plan-update`
   skill with literal arg `reconcile` before continuing.

## Step 1: Parse findings and determine scope

1. **Locate the ledger**, in order: (a) conversation context, if `<PRODUCER>` summarised it
   inline earlier in the session; (b) the resolved flow's `<LEDGER>` artifact path; (c) the
   flow-less fallback — if several candidate files exist there, list them and ask which to
   apply. **No-args-on-main special case**: empty `$ARGUMENTS`, flow-less, on a main branch
   → default to the fallback directory's `recent.toml` if present. If nothing is found, ask
   the user to run `<PRODUCER>` first. Read per the ledger-schema contract (schema_version
   handling, malformed-item skip, parse-error halt); pass `--verify-integrity` so silent
   corruption is caught before Step 5's regression cross-check trusts the bytes.

   **Empty-ledger case**: if the file is present but has no items, or zero items with
   `status = "open"`, print `ledger present at <path> but has no open items; nothing to apply`
   and exit cleanly — no further tomlctl calls beyond the initial list. An empty ledger is a
   valid outcome (either `<PRODUCER>` found nothing, or every item is already dispositioned);
   it is not an error.

2. **Selector semantics** — `$ARGUMENTS` accepts two forms, disambiguated by prefix:
   - **ID-prefixed (preferred)**: `<ID>` list — resolves against the parsed ledger's
     `[[items]]` by `id`, regardless of current disposition or report inclusion.
   - **Numeric-only (legacy)**: `1,3,5` — position in `<PRODUCER>`'s most recent emitted
     report. Resolve by filtering to items sharing the ledger's most recent `last_updated`;
     if uncertain, prompt the user to confirm which run the numbers refer to.
   - **Strong preference**: use the `<ID>` form. Numeric-only is ambiguous across disposition
     transitions (applying item 2 then re-running with `2` may select a different item).
     Recommend `<ID>` in error messages and confirmation prompts.
   - **Non-open selector behaviour**:
     - `status = "deferred"` → **hard error**: "`<ID>` is deferred. Trigger: `<defer_trigger>`.
       If the trigger has fired, run `<PRODUCER> <file>` to re-scan — the next round will
       automatically re-`open` the item if still present. If the trigger has not fired, this
       apply should wait. `<CMD>` does NOT re-open deferred items because the deferral captured
       a user-committed re-evaluation condition; bypassing it via `<CMD>` would discard that
       decision." Re-opening goes through `<PRODUCER>`'s disposition protocol.
     - `status ∈ <TERMINAL>` → **console warn and skip** (idempotent no-op). Do not re-transition.
     - Not present in the ledger → report to the user and skip.
   - **Mixed batches**: when `$ARGUMENTS` mixes valid and invalid IDs, proceed with the valid
     ones — do NOT fail-fast the whole run. Record the unknown IDs and surface them in the
     final summary's `### Unknown IDs` sub-section. Fail-fast on a mixed batch is hostile to
     users working from a stale or guessed ID list; partial success with clear reporting is
     the principle of least surprise (Google AIP-234, AWS partial-batch guidance).
   - **Override flags** (position-independent): `--file-budget <N>` (N ≥ 3) or
     `--allow-cross-file` override the default 3-file-per-item cap from the apply-constraints
     contract. Per-invocation only — no ledger mutation, no persistent state.
     - **Bare** (`--file-budget 8`, `--allow-cross-file`) applies to every item this
       invocation selects.
     - **Scoped** (`--allow-cross-file <ID>,<ID>`, `--file-budget 8 <ID>`) — the trailing id
       list narrows the override; other selected items keep the default cap.

     Honour an override by emitting one extra header line per affected cluster in the agent
     prompt, immediately after `DISPATCH:` — `FILE-BUDGET: <N | unlimited> for <id-list>`.
     Items without an override are not named in the header and inherit the default cap. The
     flags do NOT alter the lite-eligibility gate; cross-file work still routes to `implement-deep`.

3. **Selector expansion**: `"all"` → every `status = "open"` item including suggestions;
   `"critical"` → open items with `severity = "critical"`; `"critical,warnings"` → open items
   at `critical` or `warning`; empty → open critical + warning items (skip suggestions).
   Explicit selectors (ID list, numeric list, `"all"`, `"critical"`) proceed without
   confirmation; otherwise list the selected findings by `id` + `summary` and confirm first.

### Freshness gate

Fires **after** selector expansion (so the user sees only files in their resolved selector,
not the whole ledger) and **before** pre-analysis (so no Read budget is spent on
possibly-stale code).

1. Read `last_updated` from the ledger root.
2. Collect every distinct `file` referenced by items in the resolved selector.
3. For each, run `git log -1 --format=%cI -- <file>` for the newest commit timestamp on that path.
4. If any file's newest commit is at or after 00:00:00Z on the day AFTER `last_updated`, the
   ledger is stale for this selector. The comparison is UTC-based; users in non-UTC timezones
   may see staleness fire at different wall-clock times than the calendar rule suggests.

On stale detection, print a one-screen summary:

```
Ledger last_updated = <YYYY-MM-DD>; selector references files with newer commits:
  <file>  — latest commit <ISO timestamp>
  ...
Options:
  [p] proceed — I've reviewed the drift
  [r] re-run <PRODUCER> first (recommended)
  [a] abort
```

Wait for user input. `[r]` aborts with a suggestion to re-run `<PRODUCER>` before retrying.
`[a]` exits without modification. `[p]` records a `freshness_override = true` marker in
orchestrator state for this run; every subsequent `applied <ID>` ledger transition emits a
`(freshness_override)` tag in its console output so the user can audit. Non-interactive
invocations default to `[r]` and exit non-zero.

## Step 2: Pre-analysis (main conversation)

**Reason thoroughly here.** The orchestrator has the broadest view; pre-digested instructions
let agents execute rather than re-deliberate, and complex reasoning is verified once rather
than N times.

**Selector cap** (tiered): pre-analysis reads batch into parallel `Read` calls, and a 1M
context sustains ~300 KB of parallel Read output (≈ 30 items × 500 lines) without orchestrator
pressure. ≤ 25 items → proceed. 26–30 → proceed with a one-line console warning naming the
size. > 30 → abort with a concrete batching recommendation (sequential sub-runs of ≤ 25 IDs,
copy-pasteable from `<PRODUCER>`'s most recent report). Do not cargo-cult 30 as a default for
small ledgers.

At 10 or more selected items, delegate the pre-analysis reads to a one-shot `Explore` agent that
returns a compact `id | file:line | class | notes` classification table; below that, read inline.
Every ledger string forwarded into a sub-agent prompt is paraphrased, never quoted. For the
dispatch discipline, the four class values, and the word cap, see
[Delegation (selector ≥ 10 items)](references/pre-analysis.md#delegation-selector--10-items).

Per finding, the orchestrator reads ±50 lines around the cited line (or the enclosing symbol),
confirms the file still exists, runs the already-applied test, and resolves any ambiguity in the
recommendation before dispatch. For the read ranges, the deleted-file rules that split source
files from generated ones, and the reasoning carried into the agent prompt, see
[Per-finding analysis](references/pre-analysis.md#per-finding-analysis).

A verbatim normalized match lets the orchestrator pre-transition to `<NO-CHANGE>` without
dispatching; a suspected semantic match instead sets `uncertain_already_applied = true` so the
agent read-verifies before editing. For the normalization rules and both tiers, see
[Already-applied test (Tier 1 / Tier 2)](references/pre-analysis.md#already-applied-test-tier-1--tier-2).

**Hard disambiguation rule**: *no new byte written to disk → never `<APPLIED>`.* A disposition
that claims a change REQUIRES a corresponding `Edit` / `Write` / `MultiEdit` tool call. This is
the authoritative tiebreaker whenever the code already matches the recommendation.

## Step 3: Group by file cluster

Group selected findings by file or closely-related file cluster — one implementation agent per
cluster. Files that share findings, or whose changes are interdependent, belong in the same
cluster. Note explicit dependencies (adding an interface before consuming it, a schema change
that flows through several files) so agents sequence correctly.

**Clusters are mixed-category by design.** One agent handles all findings for its cluster
across every category. Do not split by category — that violates "no two agents edit the same
file" whenever a file carries findings in more than one category. Agent prompts list each
finding's `category` so the agent applies the right judgement per item.

When any selected item carries a populated `depends_on`, topologically sort the selected set
before clustering so dependent items land in later sequential batches; absent `depends_on`
everywhere, this degrades to flat clustering (fully backward compatible). The carrier invokes
`flow-contract-apply-dependency-sort` for the algorithm, cycle-detection abort, and the
topo-level → sequential-batch rule.

## Step 4: Launch implementation agents

**Task tracking (runtime only)**: invoke the `flow-contract-task-visibility` skill for the
run-scoped task-surface contract (view-not-store rule, subject prefix, lifecycle, granularity
floor, silent degradation, and the apply-flow per-batch exception). Call `TaskCreate` once per
file cluster — `subject` carries the prefix `<slug> <CMD> · c<n>` and names the cluster,
`description` lists its item IDs, `activeForm` is the present-participle form — plus one
`<slug> <CMD> · verify` task for Step 5. Move each
through `pending → in_progress → completed` with `TaskUpdate`, completing a cluster only after
its Step-4.5 vet. Do NOT mint per-finding tasks; the ledger is the persistent source of truth
for per-item state. Tasks do not persist across commands. For sequential batches, complete
batch-k's tasks before minting batch-(k+1)'s so progress reads cleanly without inter-batch
leakage.

### Lite-eligibility gate (orchestrator decision, per cluster)

Evaluate each cluster as a whole against ALL of:

1. **File scope**: ≤ 2 files.
2. **Action fully specified**: every item's `summary` + `description` names the exact change.
   No design decisions left to the implementer for ANY item in the cluster.
3. **No cross-file refactor**: no item needs coordinated edits to call sites, type definitions,
   or interfaces outside the cluster.
4. **Not security-sensitive**: no item touches auth, crypto, input-validation, sandbox-boundary,
   or token-storage code.

**Coupling-isolation rule**: if any item fails any criterion, the ENTIRE cluster goes to
`implement-deep`. Trivial items dependency-linked or file-overlapping with complex ones ride
along — cluster boundaries are NOT re-drawn for cost savings. Clean isolation outweighs the
marginal saving from peeling out trivial items.

Dispatch: passes ALL criteria → `subagent_type: "implement-lite"`; fails ANY → `implement-deep`
(the default). Record the choice as a one-line `DISPATCH:` header at the top of the agent's
prompt with its rationale, naming the failing criterion and item on a deep dispatch; the header
is captured in the execution record for audit. Append the `FILE-BUDGET:` header (Step 1) when an
override names any item in the cluster; omit it entirely otherwise.

This gate is **separate from** the critical-finding user-confirmation gate in Step 5 — that gate
suppresses silent automated `<REJECTED>` transitions, not lite/deep selection.

**Pre-dispatch summary** (console, before any Agent call): one line per cluster naming its items,
the four criteria results, and the verdict — e.g. `dispatch plan: cluster <files> (<ID>, <ID>) —
criterion 1 (≤2 files): pass; criterion 2 (fully-specified): pass; criterion 3 (no cross-file
refactor): pass; criterion 4 (not security-sensitive): pass → implement-lite`. This makes the
gate auditable from console output alone.

### Dispatch discipline

**File-cluster grouping is the primary conflict-avoidance strategy.** No two agents may edit the
same file. Findings that cannot be split into non-overlapping clusters get **sequenced, not
parallelised**; `isolation: "worktree"` is a last resort only — worktree merges are slow and risk
losing work.

**You MUST make all independent file-cluster Agent calls in a single response message.** Emit one
message containing every Agent tool-use block so they execute concurrently. **Do NOT reduce the
agent count** — launch the full complement. Dependent same-file agents run sequentially after the
parallel batch, and each sequential batch's changes are committed before the next launches, so a
later failure is revertible without losing earlier work.

`implement-lite` and `implement-deep` already carry the applied/skipped tag form, the Tier-2
already-applied protocol, the no-overlapping-edits rule, and plan-deviation reporting in their
system prompts; the per-call prompt restates only the carrier-specific vocabulary and the Step-2
pre-analysed reasoning. For the mandatory prompt elements, the obligations every agent owes, and
the partial-apply follow-up that mints a child item for the pending parts, see
[Agent prompt contract](references/agent-prompt-contract.md#agent-prompt-contract).

## Interim checkpoint

After the Step 4.5 vet (and any re-dispatched fixes), persist non-risky transitions in a single
atomic `tomlctl items apply --ops -` call. Non-risky means:

- `<NO-CHANGE>` transitions where agents wrote no bytes and reported the item already in place.
- `<REJECTED>` transitions for agent-intentional skips (no bytes written, finding declared unsafe
  or unclear).
- Orchestrator pre-transitions from Step 2 (deleted-file detection, Tier-1 already-in-place).
- Child items minted by the partial-apply follow-up — the parent's `<APPLIED>` status is deferred,
  but the child's `open` status is persistable now.

**Defer** `<APPLIED>` transitions until AFTER Step 5 passes — they depend on the build/test outcome
and on the diff reconciliation below. Defer the `last_updated` bump to the final render.

Rationale: an interrupted run (Ctrl-C between Step 4 and Step 5) would otherwise lose the
agent-reported evidence. The Step 1 idempotency guards (terminal dispositions warn-and-skip on
re-selection; missing items report-and-skip) make a re-run safe. Skip the checkpoint entirely when
no non-risky transitions are pending — do not emit an empty `--ops` payload.

## Step 5: Verification

### Step 5a: Mechanical build/test verification

Determine build and test commands from (a) CLAUDE.md's documented commands, (b) project root files
(Cargo.toml, package.json, *.sln, Makefile, pyproject.toml). Ask the user if ambiguous.

Launch the `verification` agent **once** (`subagent_type: "verification"`) with the full ordered
command list in a `commands:` field — build first, then tests, then any category-specific commands
that fit its run-and-report contract. It runs them sequentially, short-circuits on the first
`fail`, and returns one `command:` + `outcome:` block per attempted command (with `tail:` on
failure and a `not_run:` line for the remainder). Do not restate the agent's reporting contract in
the prompt — it lives in the agent's system prompt. One fan-in spawn is the supported pattern; N
single-command spawns waste orchestrator round-trips and prompt-cache hits for ~9–20 s of work each.

### Failure handling

On `outcome: fail`, **reason thoroughly to diagnose** in the main conversation. Read the affected
file(s) using the agent-supplied tail, determine root cause, then fix directly or dispatch a
targeted fix agent (`implement-deep` for non-trivial fixes, `implement-lite` if the fix is
mechanical and the lite-eligibility gate would pass). Re-run Step 5a after each attempt.

Before constructing the ledger-mutation ops, reconcile every `applied <ID>` tag against the union
of the working-tree, index, and untracked-file diffs; a claim with no corresponding diff is
downgraded to `<REJECTED>` and surfaced under `### Downgraded`. See
[Verify agent-reported `applied` claims](references/verification.md#verify-agent-reported-applied-claims).

Then run the ledger-schema dedup rule against **every** previously-`<APPLIED>` item, not only
those already chained via `related`. A match on a file touched in this run is a regression: flag
it and mint a new item under `### Regressions Triggered`. See
[Regression cross-check](references/verification.md#regression-cross-check).

Finally mutate the same ledger file consumed in Step 1, one transition per selected item, scan the
serialised payload for secrets, and write it with the two-call pattern: `items apply` for the
per-item ops, then `set` for `last_updated`. A `severity = "critical"` item in a
`<CRITICAL-CATEGORIES>` category never takes a silent `<REJECTED>`. See
[Ledger mutation](references/verification.md#ledger-mutation).

### Final summary

**Reason thoroughly through the final summary.** Cross-reference all agent results, verify
completeness, and ensure the report reflects what was actually implemented, audited, and skipped.
**Omit any sub-section with no entries.**

```
## <carrier report title>

### Implemented
- [<ID>] [file:line] [category] Summary of what was changed — (severity)
  - Tag `(partial)` for partial applies (see `resolution` for the split).
  - Tag `(chronic)` for items whose pre-apply `rounds >= 3` reached `<APPLIED>` (per the
    ledger-schema escalation rule).

### Verified Clean          # only where <NO-CHANGE> is a disposition distinct from <REJECTED>
- [<ID>] [category] Audit note

### Skipped
- [<ID>] [category] Reason — the ledger's rationale field carries the same text

### Unknown IDs
- <ID>: not present in ledger at <path> — check <PRODUCER>'s most recent output

### Downgraded
- [<ID>] [file:line] Claimed `applied` but no diff detected — transitioned to `<REJECTED>`. Investigate.

### Requires User Confirmation
- [<ID>] [file:line] [category] [severity] Agent rationale — awaiting explicit disposition before
  the ledger transition.

### Verification
- Build: pass/fail
- Tests: pass/fail/none
- Category-specific: per the carrier's checks, as applicable

### Regressions Triggered
- [<ID>] [file:line] Regression of [<old ID>] — dedup-rule match details
```

## Step 6: Plan-deviation follow-up

Inspect each agent's output for `deviation:` lines (agents emit these with the item's ledger ID —
see Step 4). Skip this step entirely if none were reported.

For each, check whether the cited file matches any `scope` glob in the resolved flow's `context.toml`
(use the `Glob` tool with the flow's `scope` patterns).

- **In-scope**: auto-invoke the `plan-update` skill via the `Skill` tool with the literal argument
  `deviation`, passing through the agents' details (item ID, file, applied-fix summary) so
  `plan-update deviation` can record them.
- **Out-of-scope** (no matching glob, or no flow resolved): no plan update fires, so capture the
  deviation in the repo-scoped backlog instead of leaving it in prose. Invoke the `backlog-capture`
  skill for the capture discipline before the first mint of the run. Per deviation, run the mandatory
  gate with the same `--kind` and `--area` the mint will use:

  ```bash
  tomlctl backlog check --summary "<deviation summary>" --kind debt --area "<deviation file>"
  ```

  Act on the verdict — `duplicate`, `previously-resolved` and `duplicate-id` mint nothing;
  `likely-duplicate` mints only after reading the named candidate; `related` mints with
  `--related "<candidate id>"`; `novel` mints:

  ```bash
  tomlctl backlog add --summary "<deviation summary>" --kind debt --area "<deviation file>" --context "<the deviation's rationale>" --origin <CMD> --flow "<slug>"
  ```

  `--origin` takes the bound carrier command name — `review-apply` or `optimise-apply`, since this
  contract is shared by both. Report each deviation in the final summary with the item ID, file path,
  applied fix, the note that it falls outside the active flow's scope, and the backlog id it minted
  (or the verdict that suppressed the mint).

### Capped-skip capture

Runs independently of the deviation gate above — whenever a capped-skip tag came back, including
runs where no `deviation:` line did.

The apply-constraints contract makes an agent emit `skipped <ID>: cross-file refactor exceeds 3-file
cap` and `skipped <ID>: requires deliberate refactor` when an item is real but too large for its
dispatch. Unless the orchestrator writes a `<REJECTED>` transition carrying that reason, the work
disappears with the run's prose. Route each such tag into the backlog after the ledger mutation,
gate first:

```bash
tomlctl backlog check --summary "<the ledger item's summary>" --kind debt --area "<the ledger item's file>"
```

```bash
tomlctl backlog add --summary "<the ledger item's summary>" --kind debt --area "<the ledger item's file>" --context "<the skip reason>" --origin <CMD> --flow "<slug>"
```

Never `add` before `check`. The orchestrator is the only writer here — cluster agents and the
`verification` agent never touch the store; they surface candidates in their return payload and the
orchestrator mints them.

### Sync plan context

After Steps 5 and 6 complete, synchronise the resolved flow's `context.toml`.

1. **No-op gate**: skip entirely if no flow resolved, OR if no agent wrote bytes to any file matching
   the flow's `scope` globs.
2. Otherwise invoke `Skill("plan-update", "status")` — it refreshes `context.updated` and updates
   `[tasks]` counters when apply-time transitions affect tracked plan tasks. `plan-update` performs
   its own flow resolution, so no arguments pass through.

## Constraints

The apply-constraints contract bounds every cluster agent's edits; the carrier invokes it directly.
Two rules apply to the orchestrator rather than the agents:

- **Public API or schema changes** require explicit user confirmation. Agents emit
  `skipped <ID>: requires user confirmation on public API / schema change` and the orchestrator
  surfaces the decision rather than letting an agent apply unilaterally.
- **Do NOT handle `deferred`-forward transitions.** Deferral requires a user-committed
  re-evaluation trigger; `<PRODUCER>`'s disposition protocol owns that surface.
