# tomlctl — review apply reference

The `tomlctl review` verbs that fix code and perf findings through a v2 flow: scheduling fix and settle tasks, recording a settle proposal's outcome, reading task state back onto the findings, and the build-free checks a fix task's premises and accept checks ask. The store shape, the shared flag bundles, payload sources and output conventions are [review.md](review.md)'s, under Shared behaviour and Store shape; this file holds only these verbs. Which findings are applied and how the fix milestone runs are the `apply-v2-engine` and `implement-v2-engine` skills' call, not this one's.

## Contents

- [Apply through a v2 flow](#apply-through-a-v2-flow) — the `[[finding_links]]` table and the chain from review store to task store
- [`schedule`](#tomlctl-review-schedule)
- [`settle`](#tomlctl-review-settle)
- [`reconcile`](#tomlctl-review-reconcile)
- [`anchor`](#tomlctl-review-anchor)
- [`reproduce`](#tomlctl-review-reproduce)
- [`reproduce-status`](#tomlctl-review-reproduce-status)

## Apply through a v2 flow

Code and perf findings can be fixed inside the v2 chain instead of a v1 ledger: `schedule` mints fix tasks into a v2 flow's `tasks.toml`, `/implement-v2` runs them, and `reconcile` reads each task's state back onto its findings. A task names its findings through the task store's `[[finding_links]]` table (`ref`, `store`, `fixes`, `settles`; the flag and row keys are in [tasks-write.md](tasks-write.md), the shape in [tasks-store.md](tasks-store.md)), where `store` is the review store's repo-relative path. `anchor` and `reproduce-status` answer by exit code, so a minted task's premises and accept checks can ask them.

### `tomlctl review schedule`

Mints fix tasks in the `--flow` flow's task store from open defect and cleanup findings and marks each finding `scheduled`, with `apply_flow` and `apply_task`. The flow must hold `plan.toml`; a v1 flow is refused, naming `export-v1` as its route.

- **Selection.** `export-v1`'s rule, run over the `code` and `perf` families: an explicit id is refused for the plan family, advisory, any status but `open`, a blocked finding ([Blocked findings](review.md#store-shape)) or `effort = large`. `--all` takes every open code or perf defect and cleanup that is not blocked, holds back test removals and cleanup whose latest verdict is `plausible` under `held` (those schedule only by explicit id), and lists each blocked one under `refused`. When every explicit id is refused and no settle task is due, the call fails `kind=validation` and writes nothing.
- **Blocked findings.** An open finding blocked on `evidence` that carries no `proposal` gets one settle task when `--settle-checkpoint` is given, and is refused naming the flag otherwise; one that already carries a `proposal` is refused, since a finding has one settle round. `measurement` and `alternatives` blocks are refused as waiting for trials, and `decision` and `design` blocks with the export rule's reason. A settle task joins `--settle-checkpoint`, runs on `deep`, holds one new file, `docs/flows/<flow>/notes/settle/<id>.md`, and its outcome and build-free accept check are that the note's first line opens `Proposal:` or `Refuted:`. It is linked under `settles`, and its finding is marked `scheduled` like a fix task's.
- **Answered blocks.** A finding's `decision_answer` binds its fix task as a `Decision: <answer>` constraint (`Decision on <id>: <answer>` on a task holding several findings), and a `fix_sketch` is appended to that finding's part of the `detail`.
- **Grouping.** Findings whose edited paths (`target_file` or `file`, and every `instances` path) overlap, transitively, share one task. Each task carries `checkpoint = --checkpoint`, which must be a group the store declares; `files` from the edited paths; `needs` from each finding's `depends_on`, mapped to the task holding the target (an id that maps to none lands under `dropped_deps`); effort `M` when any finding's `effort` is `medium`, else `S`; an `action` line per finding (`<id>: <summary>`) and a `detail` built from each finding's implementer projection; and a spec. The spec's `outcome` states each finding gone, its premises check `review anchor <F>` at base, and its accept checks are `review anchor <F> --gone` (build-free) for a `remove` cleanup with `old`, and `review reproduce-status <F> --expect gone` for every other finding. The executor is `lite` when the task holds at most two files and every finding is a cleanup or of `trivial` or `small` effort, else `deep`. Each row carries `reason = "apply <store>: <ids>"`, so an audited store accepts it.
- **Writes.** Rows are minted under the task store's lock through `tasks add-many`'s own parse and refusals, each with its `[[finding_links]]` entry; the review store is written after. A finding already linked to a live task — one with no `commit` and not `deferred` — is placed on that task as `linked` rather than minted again, so a rerun after a failure between the two writes finishes the job. A finding that left `open` before its mark lands under `skipped_stale`.

Prints `{ok, flow, review_store, scheduled: [{finding, task, ref, action}], settle, held, refused, dropped_deps, skipped_stale}`, where `action` is `minted` or `linked`, and `settle` lists the settle tasks in the same shape. Settle rows follow the fix rows, and a finding a live settle task already `settles` is `linked` to it.

- **Dry run.** `--dry-run` runs the same selection, grouping and executor rule and writes neither store. It prints the same object with `dry_run: true` and no `skipped_stale`; a row a real run would mint is `{finding, task, action: "would-mint", title, executor, files}`, `task` being the id it would take. `--flow` and `--checkpoint` may be left out before the flow exists: ids then count from 1, and nothing is `linked`. An evidence-blocked finding with no `proposal` is listed under `settle` whether or not `--settle-checkpoint` is given, and a selection refused whole is reported under `refused` with exit 0.

```bash
tomlctl review schedule F3,F7 --scope <scope> --flow <apply-slug> --checkpoint A
tomlctl review schedule --all --slug <slug> --flow <apply-slug> --checkpoint A --settle-checkpoint A
tomlctl review schedule --all --scope <scope> --dry-run
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids | Finding ids, comma- or space-separated. Excludes `--all`. | required without `--all` |
| `--slug` / `--scope` | name | The review store. | required, one of |
| `--all` | bool | Every schedulable open code or perf defect and cleanup in this store. | off |
| `--flow` | slug | The v2 flow whose `tasks.toml` receives the fix tasks. | required without `--dry-run` |
| `--checkpoint` | id | The checkpoint group every minted task joins. | required without `--dry-run` |
| `--settle-checkpoint` | id | The checkpoint group a settle task joins; without it an evidence-blocked finding is refused. | none |
| `--dry-run` | bool | Report what would be scheduled, held and refused; write nothing. | off |

### `tomlctl review settle`

Records what became of a settle task's proposal for one finding blocked on `evidence`, at `open` or `scheduled`; exactly one outcome per call. Every outcome sets the finding `open` or `refuted` and clears `apply_flow` and `apply_task`.

- **`--accept`** with `--tier` and `--fix-file`: clears the block and records `fix_sketch` (the file's text, at most 800 characters), `settle_tier` and `unblocked_by = "settle"`, and drops any `proposal`; status → `open`, fixable as written.
- **`--refute`** with `--citation`: records an `orchestrator` `refuted` verdict through `verdict`'s checks; status → `refuted`, the block kept. It decides at once on any route, a panel-routed finding included, as an audit verdict does.
- **`--defer`** with `--proposal`: records the note's repo-relative path as `proposal` and the time as `proposal_at`, leaving the finding `open` and blocked, where it ages ([Deferred proposals](review.md#store-shape)). A note that is missing or outside the repository is refused, and a deferral is never renewed: a finding already carrying a `proposal` is refused until it is accepted or refuted.

A finding at another status, or not blocked on `evidence`, is refused with its reasons and nothing is written; an outcome already recorded writes nothing. Prints `{ok, id, from, status, outcome, unchanged}`, where `outcome` is `accepted`, `refuted` or `deferred`.

```bash
tomlctl review settle F5 --slug <slug> --accept --tier cited --fix-file <run-dir>/payloads/F5-fix.txt
tomlctl review settle F6 --slug <slug> --defer --proposal docs/flows/<flow>/notes/settle/F6.md
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | finding id | The finding blocked on evidence. | required |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--accept` | bool | Unblock the finding with the proposal's fix; needs `--tier` and `--fix-file`. | one outcome required |
| `--refute` | bool | Refute the finding; needs `--citation`. | one outcome required |
| `--defer` | bool | Record the proposal note and leave the finding blocked; needs `--proposal`. | one outcome required |
| `--tier` | `mechanical` \| `cited` \| `user` | How the accepted proposal was settled; with `--accept` only. | none |
| `--fix-file` | path | The fix the proposal settled on, recorded as `fix_sketch`; with `--accept` only. | none |
| `--citation` | text | What shows the finding false; with `--refute` only. | none |
| `--proposal` | path | The proposal note, a file in the repository; with `--defer` only. | none |

### `tomlctl review reconcile`

Moves each finding `scheduled` into `--flow` by its task, found by `apply_task`; when no row carries that ref, the row with the finding's `apply_task_id` that a `[[finding_links]]` entry of this store still ties to the finding is the same task renamed, and `apply_task` is rewritten (listed under `retitled`). An earlier, committed task of a finding scheduled twice has another id and is never followed. A finding stored without `apply_task_id` follows the last such link. By the task's state:

| Task | Finding |
|---|---|
| `done` with a `commit`, the finding under `fixes` | `resolved`, with `resolution = "<flow> <ref> <commit>"` and today's `resolved`; the apply link is kept |
| `done` with a `commit`, the finding under `settles` | `open`, as no proposal is recorded |
| `deferred`, or no row left | `open`; the apply link is cleared |
| anything else | still `scheduled`, listed under `in_flight` |

Every move to `open` writes `reopen_reason = "apply: <why>"`, the prefix `stats --apply` counts by. Only the review store is written, and only when a finding moved. A store holding a finding scheduled into the flow while the flow has no `tasks.toml` fails `kind=validation`.

Prints `{ok, flow, review_store, resolved: [{finding, task, commit}], reopened: [{finding, task, reason}], in_flight: [{finding, task, status}], retitled: [{finding, from, to}]}`.

```bash
tomlctl review reconcile --slug <slug> --flow <apply-slug>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The review store. | required, one of |
| `--flow` | slug | The v2 flow whose `tasks.toml` holds the scheduled findings' tasks. | required |

### `tomlctl review anchor`

Exits 0 when the finding's anchor holds in the working tree and 1 when it does not, printing `{ok, id, kind, gone, reason}` either way (`reason` is `null` when it holds). Writes nothing.

- **cleanup** with `old`: `old` is found in its edit target (`target_file` when it exists, else `file`) exactly as `review add` matches it, within 20 lines of `line`. Without `old`, the file exists.
- **defect**: `file` exists and, when `symbol` is set, the symbol's last segment (after its last `::`, `.` or `/`) occurs in it as a whole word.
- **advisory**: refused `kind=validation`, having no anchor.
- **`--gone`**: the opposite check for a `remove` cleanup: `old` occurs nowhere in its edit target, or the target is gone. Any other finding, or one without `old`, is refused `kind=validation`.

An unknown id fails `kind=not_found`.

```bash
tomlctl review anchor F3 --slug <slug>
tomlctl review anchor F9 --scope <scope> --gone
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | finding id | The finding checked. | required |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--gone` | bool | For a `remove` cleanup: exit 0 when `old` occurs nowhere in its edit target. | off |

### `tomlctl review reproduce`

Records reproduce verdicts — what a check found of a finding after its fix landed — in `[[reproductions]]` ([Store shape](review.md#store-shape)), apart from `[[verdicts]]`: no reproduction changes a finding's status, and no verdict reader reads them. Each NDJSON row carries `finding` (in this store), `flow`, `milestone`, `round` (a positive integer), `verdict` (`gone`, `reproduces` or `unsettled`), `by` (`verifier` or `orchestrator`) and `citation`, plus an optional `task`, the apply task's ref; any other key is refused, and `at` is stamped today.

A row is upserted on (finding, `flow`, `milestone`, `round`, `by`): a changed row replaces its predecessor at the end of the array, where a finding's current reproduction — its last row — is read from; an identical one, `at` aside, keeps its place and writes nothing, so replaying an earlier round never makes it current again. Prints `{ok, reproductions: [{finding, flow, milestone, round, by, verdict}], refused: [{line, finding, reasons}], unchanged}`.

```bash
tomlctl review reproduce --slug <slug> --ndjson <run-dir>/payloads/reproduce.ndjson
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--ndjson` | `-` \| path | Reproductions, one JSON object per line. | required |

### `tomlctl review reproduce-status`

Exits 0 when every named finding's current reproduction has the `--expect` verdict and 1 otherwise; a finding with no reproduction does not match. Prints `{ok, expect, findings: [{id, matches, verdict, flow, milestone, round, by}]}`, with `verdict: null` and only `id` and `matches` beside it for a finding with none. An unknown id fails `kind=not_found`, and an `--expect` outside the vocabulary `kind=validation`. Writes nothing.

```bash
tomlctl review reproduce-status F3,F7 --slug <slug> --expect gone
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids | Finding ids, comma- or space-separated. | required |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--expect` | `gone` \| `reproduces` \| `unsettled` | The verdict every current reproduction must have. | required |
