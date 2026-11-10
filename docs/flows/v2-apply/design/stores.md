# Stores

What the review store and the task store hold for apply. Field and verb names here are the contract between tasks; internal layout is the implementing task's choice.

## Finding status and link

- A new finding status, `scheduled`: the finding has a task in an apply flow. Triage cannot move it and a verdict does not decide it.
- Two link fields on the finding, `apply_flow` (the flow slug) and `apply_task` (the task row's `ref`). They are link fields, not status companions: they survive the move to `resolved`, and are cleared when the finding returns to `open`.
- Transitions:

| From | To | When | Written by |
|---|---|---|---|
| `open` | `scheduled` | its task is minted | `review schedule` |
| `scheduled` | `resolved` | the linked task is `done` with a non-empty `commit` | `review reconcile` |
| `scheduled` | `open` | the linked task row no longer exists, or is `deferred` | `review reconcile` |

- A linked task that is `failed`, `parked`, `pending`, `in-progress`, or `done` without a `commit` leaves the finding `scheduled`: in flight, never resolved.
- `resolved` carries the existing companions, with `resolution` naming the flow, the task ref and the commit sha.
- A return to `open` sets `reopen_reason`, opening `apply: ` and saying why (the task row was removed, or a settle task was committed with no recorded proposal). That prefix is how a reader counts findings apply reopened.
- A later run that re-finds a `scheduled` finding is a repeat, as for `exported`.

## Task-side link

`tasks.toml` gains `[[finding_links]]`, shaped like `[[backlog_links]]` and keyed by the task row's `ref`:

```toml
[[finding_links]]
ref = "fix-the-retry-bound"
store = ".claude/review-v2/auth.toml"
fixes = ["F3", "F7"]
settles = []
```

- `store` is the review store's repo-relative path.
- `fixes` are findings the task's work resolves. `settles` are findings the task investigates and proposes a fix for.
- `tasks add-many` accepts the row keys `fixes`, `settles` and `review_store`. Removing a task row removes its link entry. An entry with no `fixes` and no `settles` is not stored.

## Reproductions

The review store gains a top-level `[[reproductions]]` array and supports `schema_version = 2`. A store is written as version 2 only when it holds a reproduction row or a `scheduled` finding; any other store keeps version 1 on write, so a binary that predates apply can still read it. A version-1 store always loads.

```toml
[[reproductions]]
finding = "F3"
flow = "auth-apply"
milestone = "B"
round = 1
task = "fix-the-retry-bound"
verdict = "gone"
by = "verifier"
citation = "src/retry.rs:40"
at = 2026-11-12
```

- `verdict` is `gone`, `reproduces` or `unsettled`. `by` is `verifier` or `orchestrator`.
- The key is `(finding, flow, milestone, round, by)`. A write with the same key removes the old row and appends the new one, and any other write appends, so the array is in write order.
- A reproduction never changes the finding's status and is read by no reader of `[[verdicts]]`.
- The current reproduction of a finding is its last row in the array.

## Blocked findings

`fix_shape` leaves the field catalogue. A defect or cleanup is fixable unless it carries `blocked_on`:

| `blocked_on` | Required with it |
|---|---|
| `decision` | `blocked_missing`; `blocked_options`, at least two |
| `evidence` | `blocked_missing`, the exact unknown |
| `measurement` | `blocked_missing`; `blocked_metric`; `blocked_command` |
| `alternatives` | `blocked_missing`; `blocked_options`, at least two |
| `design` | `blocked_missing`; `blocked_contract`, the contract change across files or milestones |

- `review add` and `review triage --convert` refuse a row that sets `blocked_on` without what its row requires, and refuse a row that supplies `fix_shape`, naming `blocked_on`. The agent schema from `review schema finding` states the same conditions.
- A decision option names its consequence in the same string.
- **Legacy rows.** A stored row with `fix_shape = "needs-plan"` and no `blocked_on` reads as `blocked_on = "design"` everywhere a reader asks whether a finding is blocked. Nothing is required of it on load.
- The verifier projection shows `blocked_on` and its detail fields; `fix` stays withheld.

### Unblocking

| How | Result on the finding |
|---|---|
| A verifier verdict with `unblocks = true` and `fix_sketch` | `blocked_on` and its detail cleared, `fix_sketch` kept, `unblocked_by = "verifier"` |
| `review triage <id> --answer-file <path>` on a `decision` finding | `blocked_on` and its detail cleared except `blocked_options`, `decision_answer` set, `unblocked_by = "user"` |
| `review settle <id> --accept` | `blocked_on` and its detail cleared, `fix_sketch` set from the proposal, `settle_tier` set, `unblocked_by = "settle"` |

`fix_sketch` is shown to the implementer beside `fix`.

### Proposals

A settle task ends in a proposal note. `review settle` records what became of it:

| Call | Result |
|---|---|
| `--accept --tier mechanical\|cited\|user --fix-file <path>` | unblocked as above; status `open`, ready to schedule |
| `--refute --citation <text>` | status `open` first, then an `orchestrator` verdict `refuted` with the citation, which decides the status |
| `--defer --proposal <note path>` | status `open`, still blocked, `proposal` and `proposal_at` set; `proposal_at` is a UTC datetime |

- The settle drain meets the finding `scheduled`, linked to its settle task. Every outcome takes it off `scheduled` and clears `apply_flow` and `apply_task`.
- A finding has one settle round: `review schedule` refuses to mint a settle task for a finding that carries `proposal`, and `review settle --defer` is refused on one that already does, so a deferral cannot be renewed.
- A deferred proposal has its disposition when `review settle --accept` or `--refute` records one, or when triage moves the finding off `open`.
- The age of a deferred proposal is the number of runs of its family in the store whose `started` is later than `proposal_at`. At an age of 3 it is overdue. `review digest` lists deferred proposals with their age and marks the overdue ones.
