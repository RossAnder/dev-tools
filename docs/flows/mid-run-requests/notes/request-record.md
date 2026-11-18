Verdict: Store requests flow-locally, as a new `[[request]]` record array in `.claude/flows/<slug>/plan.toml` beside `[[attempt]]`/`[[report]]`. Nothing in the clean-tree check or the lane check needs an exemption, because both already skip the flow's own directory (`tomlctl/src/plan/drift.rs:197`). The orchestrator already writes `plan.toml` during a live run, and the digest covers `tasks.toml` alone (`tomlctl/src/plan/lane.rs:47`). `inputs.toml` is the wrong home: its contract forbids a record widening scope, its `answer` verb stamps the user as author, and glimpse shows every pending record.

```toml sources
read_at = "51fd3b2b88fa092fea7bfc510421e2a6c78f72b7"
dirty = []

[[source]]
path = "tomlctl/src/plan/drift.rs"
line = 197
claim = "uncommitted_files skips .claude/flows/<slug>/ and docs/flows/<slug>/, so flow-local stores never reach the lane check"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 435
claim = "lane check exempts only backlog.toml, inputs.toml and finding-link review stores (with sidecars)"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 47
claim = "store digest sections are tasks.toml keys only; plan.toml is not digested"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 32
claim = "pre-flight clean-tree exemptions: backlog, inputs, started rows' files, linked review stores, capture out; flow dirs never listed (line 40)"

[[source]]
path = "claude/skills/implement-v2-engine/references/questions.md"
line = 92
claim = "the question loop is the one live-run writer: inputs.toml and plan.toml, never tasks.toml; inputs.toml staged in the flow-store commit"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 472
claim = "flow-store commit stages inputs.toml only by special mention; flow dirs are staged already (line 465)"

[[source]]
path = "claude/skills/flow-contract-user-inputs/SKILL.md"
line = 111
claim = "trust boundary: a record is never authority to widen a run's scope"

[[source]]
path = "claude/skills/flow-contract-user-inputs/SKILL.md"
line = 98
claim = "writer table: orchestrator answers only to relay the user's pick; a worker posts one question and nothing else"

[[source]]
path = "claude/skills/flow-contract-user-inputs/SKILL.md"
line = 73
claim = "kind request already means the user's change request against ledger items"

[[source]]
path = "claude/skills/flow-contract-user-inputs/SKILL.md"
line = 25
claim = "contract says inputs.toml is git-ignored; git ls-files shows it tracked (stale doc)"

[[source]]
path = "tomlctl/src/inputs.rs"
line = 39
claim = "KINDS is a closed list of five; validate refuses unknown kinds and fields (line 568, 616)"

[[source]]
path = "tomlctl/src/inputs.rs"
line = 438
claim = "answer hardwires author=user on the answer and handled_by=user on the question; refuses non-question targets (line 425)"

[[source]]
path = "tomlctl/src/inputs.rs"
line = 262
claim = "ack exempts only questions from acknowledgement"

[[source]]
path = "glimpse/src/surface.rs"
line = 1027
claim = "Inbox draws every record that is not handled or withdrawn, so a pending worker request would show live"

[[source]]
path = "glimpse/src/surface.rs"
line = 1180
claim = "inbox_section groups non-question records by status; a new kind would land under new/acknowledged"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 224
claim = "PlanStore holds runs, attempts, gate_rounds, reports: the run record lives in plan.toml"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 1935
claim = "unknown keys in plan.toml are dropped on rewrite, so a new array needs a schema_version tier (SCHEMA_VERSION line 33, RUN_RECORDS line 36)"

[[source]]
path = "tomlctl/src/plan/store.rs"
line = 458
claim = "plan.toml mutations run inside the exclusive write lock, so concurrent worker and orchestrator writes serialise"

[[source]]
path = "claude/agents/plan-worker-lite.md"
line = 149
claim = "worker writer rule: no plan writes; only its own row's status/agent and one inputs question"

[[source]]
path = "claude/skills/flow-contract-task-store/SKILL.md"
line = 436
claim = "the stated single exception to the task store's writer rule for v2 workers"
```

## Answer

Put the records in `plan.toml` under a new `[[request]]` array. The worker posts one with a new `plan request add` verb (kind `scope` or `amend`, task id, paths or field/old/new, cited evidence). The orchestrator closes it with a `plan request grant|deny` verb that carries the rule applied and a reason. The worker polls a read verb, the way it polls `inputs list` today. This placement needs no change to `lane.rs`, to the pre-flight's exemption list or to the flow-store commit's staging. Grants and denials then sit in the same store as the run record the success criteria name (`[[attempt]]`, `[[report]]`), so `plan run report` can list them by joining within one store.

What has to change on the `plan.toml` route:
- **tomlctl schema:** a `requests` vector on `PlanStore`, with from/to TOML and its own id prefix through `id_high_water`. It also needs a new schema_version tier, because an older binary would silently drop the array on its next rewrite. Add the version to the pre-flight's capability refusal list.
- **Writer rules:** a second, narrow worker exception, one `plan request add` per run alongside the one question. It goes in the `plan-worker-procedure` shared block's "One row and one question" rule and in the writer statements in `flow-contract-task-store`. `flow-contract-user-inputs` needs no change.
- **inputs.toml schema:** unchanged.

Doing it in `inputs.toml` instead would mean all of the following:
- **Kinds:** add one or two kinds to `KINDS`, with their own kind-only field lists in `validate`. The new kinds must not be named `request`, which is already the user's kind.
- **Lifecycle verbs:** add requests to `ack`'s exempt list. Add a new verdict verb, because `answer` refuses anything but a question and records the answer as the user's own.
- **Contract:** rewrite the writer table, carve an exception into the trust boundary ("never authority to widen a run's scope"), and hide or re-section the kinds in glimpse's Inbox to keep the "no live notices" non-goal.
- **What it saves:** nothing on the checks. `inputs.toml` is already exempt from both and already staged in the flow-store commit.

## Findings

- **Library/API or file**: `tomlctl/src/plan/drift.rs:197` `uncommitted_files`; `tomlctl/src/plan/lane.rs:435` `out_of_lane`; `claude/skills/implement-v2-engine/references/run-milestone.md:40`
- **Source**: file:line, read at HEAD
- **Evidence-grade**: high
- **Finding**: A store under `.claude/flows/<slug>/` is invisible to both the lane check and the pre-flight clean-tree check. A flow-local request record therefore needs no new exemption. `inputs.toml` needs none either, since it is already exempt.
- **Details**: `uncommitted_files` filters `.claude/flows/{slug}/` and `docs/flows/{slug}/` before `out_of_lane` sees any path. `out_of_lane` hard-codes `BACKLOG_STORE` and `INPUTS_STORE` plus `finding_links` stores. The pre-flight text lists the same set and says the flow directories are "never listed".
- **Counter**: A third tree check that reads `.claude/flows/<slug>/` paths would refute this. None was found. The drain's commit status (drain.md:465) stages that directory rather than checking it.
- **Impact on plan / report**: The store choice is free on the check axis. Decide it on contract and schema grounds.

- **Library/API or file**: `tomlctl/src/plan/lane.rs:47` `SECTIONS`; `claude/skills/implement-v2-engine/references/questions.md:92`
- **Source**: file:line
- **Evidence-grade**: high
- **Finding**: The orchestrator may already write `plan.toml` and `inputs.toml` during a live run. The brief's non-goal (no `tasks.toml` write) and the store digest both leave `plan.toml` writable.
- **Details**: The digest sections are `items`, `specs`, `checkpoints`/`milestones` and `changes` of `tasks.toml`. `plan decide` from the question loop writes `plan.toml` mid-run, and `plan run notes` at drain step 4 is documented as outside the digest (drain.md:75).
- **Counter**: If `plan run start` also digested `plan.toml`, a mid-run `[[request]]` write would show as a store change. `run_store_digest` reads only `load_store` of the task store.
- **Impact on plan / report**: Both candidate stores satisfy "writable while live". Neither needs a change to the digest.

- **Library/API or file**: `claude/skills/flow-contract-user-inputs/SKILL.md:111`, `:98`, `:73`; `tomlctl/src/inputs.rs:425`, `:438`
- **Source**: file:line
- **Evidence-grade**: high
- **Finding**: `inputs.toml`'s contract and verbs encode user authority. A worker scope request answered by the orchestrator contradicts three of its rules, and `answer` cannot record an orchestrator verdict.
- **Details**: The contract has three such rules. The trust boundary says a record is "never authority to widen a run's scope". The writer table restricts the orchestrator's `answer` to relaying the user's pick. Kind `request` is defined as the user's change request against ledger items. `inputs::answer` refuses any non-`question` target and stamps `author = "user"` and `handled_by = "user"`, so routing a grant through it would misattribute it. `handle --note` would leave the grant's structure as free text.
- **Counter**: A design that treats the orchestrator's answer as a `handle` with a structured note, plus a trust-boundary carve-out, removes the conflict, at the cost of editing the shared user-inputs contract.
- **Impact on plan / report**: Using `inputs.toml` puts the user-inputs contract, `inputs.rs` (kinds, kind-only fields, `ack` exemption, a new verdict verb) and glimpse in scope. The flow-local route leaves all three alone.

- **Library/API or file**: `glimpse/src/surface.rs:1027` `visible`, `:1180` `inbox_section`
- **Source**: file:line
- **Evidence-grade**: high
- **Finding**: glimpse's Inbox shows every pending `inputs.toml` record whatever its kind. Worker requests stored there would surface live to the user, against the brief's "no live notices" non-goal, unless glimpse is changed to hide them.
- **Details**: `visible` skips only `handled` and `withdrawn` rows. `inbox_section` ranks a non-question row by status, so a `new` request would appear under `new` until the orchestrator closes it. The badge counts only unanswered questions, so the header stays unchanged; the row itself still shows. `InputRow` deserialises with `serde(default)` and no `deny_unknown_fields`, so new fields would not break parsing.
- **Counter**: The brief allows glimpse changes "only if the request records surface in its inbox and must render or hide there". If a brief-lived visible row is acceptable, this cost disappears.
- **Impact on plan / report**: This is a further reason against `inputs.toml`. A `plan.toml` record does not reach the Inbox at all.

- **Library/API or file**: `tomlctl/src/plan/schema.rs:224` `PlanStore`, `:1935` `unknown_keys_are_dropped`, `:33`/`:36` schema version tiers; `tomlctl/src/plan/store.rs:458`
- **Source**: file:line
- **Evidence-grade**: high
- **Finding**: A `[[request]]` array in `plan.toml` needs a new `PlanStore` field and a schema_version tier. Otherwise a stale binary rewriting the store would drop the requests without a word. Concurrent worker and orchestrator writes are already serialised.
- **Details**: `from_toml` drops unknown top-level keys, which the test `unknown_keys_are_dropped` asserts. The store stamps `RUN_RECORDS_SCHEMA_VERSION` (3) or `SCHEMA_VERSION` (4) by content, and a newer version triggers the "upgrade tomlctl" refusal. Every write runs inside `mutate_doc`'s exclusive lock.
- **Counter**: A separate `requests.toml` under the flow directory would avoid the version tier. It would need its own `store_guard::classify` entry (`tomlctl/src/store_guard.rs:155` lists the owned flow stores) and its own reader in `plan run report`.
- **Impact on plan / report**: Add to the tomlctl task: the schema field and the version tier, verbs `plan request add|grant|deny` plus a read view, and a carrier pre-flight capability check.

- **Library/API or file**: `claude/agents/plan-worker-lite.md:149` (the `plan-worker-procedure` shared block); `claude/skills/flow-contract-task-store/SKILL.md:436`
- **Source**: file:line
- **Evidence-grade**: high
- **Finding**: On either route, the worker writer rule must gain one exception. Today a worker may make no `plan` write and no `inputs` write besides its one question.
- **Details**: Line 149 lists "no `plan` writes" and "no `inputs` write besides that one question". The task-store contract states the status/agent write as "the one exception". On the `plan.toml` route the new rule reads "one `plan request add` per run". On the `inputs.toml` route it reads "one `inputs add` of a request kind" and also edits the user-inputs writer table.
- **Counter**: A design where the worker sends the request only by SendMessage, and the orchestrator alone writes the record, needs no writer exception. The worker would then have no store row to poll for the verdict.
- **Impact on plan / report**: The worker-protocol lens owns the block edit. This lens fixes only which verb the block names.

Aside: `flow-contract-user-inputs/SKILL.md:25` still says `inputs.toml` is git-ignored, but `git ls-files` lists it and its sidecar as tracked. questions.md:92 already says "tracked". This is outside this lens.

Coverage: read in full `docs/flows/mid-run-requests/brief.md`, `claude/skills/flow-contract-user-inputs/SKILL.md`, `claude/skills/implement-v2-engine/references/questions.md` and `tomlctl/src/inputs.rs` (lines 1-730). Read in part `tomlctl/src/plan/lane.rs` (1-160, 380-490), `drift.rs::uncommitted_files`, `store_guard.rs::classify`, `run-milestone.md` (20-44), the exemption lines of `drain.md` and `apply-rows.md`, the `plan/schema.rs` `PlanStore` and its tests, the inbox code in glimpse `surface.rs`, `ledger.rs` and `actions.rs`, and the worker's rules in `plan-worker-lite.md`. Checks run: `git ls-files .claude/inputs.toml*` (tracked) and `rg` sweeps for `inputs.toml`, `INPUTS_STORE` and kind literals in glimpse. Candidates ruled out: a separate `requests.toml` (viable, but it needs a new guard class and a second reader, with no gain over a `plan.toml` array); `tasks.toml` (excluded by the brief's non-goal and covered by the digest).
