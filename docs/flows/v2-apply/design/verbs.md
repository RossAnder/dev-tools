# Verbs

The `tomlctl` surface apply adds. Every verb takes the review group's store flags (`--slug <slug>` or `--scope <scope>`) and its integrity flags; write verbs take `--dry-run`. Output is the group's usual JSON envelope.

## review anchor

`tomlctl review anchor <id> [--gone]` — a read. Exit 0 when the finding's anchor holds in the working tree, 1 when it does not.

- Cleanup: the anchor holds when `old` is present at the edit target, by the same matching `review add` applies when it accepts the row. A cleanup with no `old` holds when its `file` exists.
- Defect: the anchor holds when `file` exists and, when the finding has a `symbol`, the symbol's last path segment occurs in the file.
- `--gone` is for a `remove` cleanup: exit 0 when `old` occurs nowhere in the edit target file. It searches the whole file, not the window around `line`, so text an edit only shifted is not read as gone. It is refused for a defect, an `add` and a `change`, whose new text may keep `old`.

## review reproduce and review reproduce-status

- `tomlctl review reproduce --ndjson <path>` writes `[[reproductions]]` rows (`finding`, `flow`, `milestone`, `round`, `task`, `verdict`, `by`, `citation`); see [stores.md](stores.md#reproductions). It refuses an unknown finding, a verdict outside the vocabulary and a row with no `citation`.
- `tomlctl review reproduce-status <id>[,<id>...] --expect gone|reproduces|unsettled` — a read. Exit 0 when the current reproduction of every named finding has that verdict, 1 otherwise, including when a finding has no reproduction.

## review schedule

`tomlctl review schedule <id>[,<id>...] | --all --flow <slug> --checkpoint <id> [--settle-checkpoint <id>]` mints task rows in the flow's `tasks.toml` and marks each finding `scheduled`.

- **Selection** is the rule set `export-v1` uses, shared with it: only `open` defect and cleanup findings of the code and perf families; `effort = large` is refused; `--all` also holds back a cleanup whose latest verdict is `plausible` and a test removal, which an explicit id still schedules.
- **Blocked findings.** A finding with `blocked_on = evidence` and no `proposal` becomes a settle task in `--settle-checkpoint`, and is refused when that flag is absent. Every other blocked finding is refused with its `blocked_on` as the reason.
- **Folding.** Selected fixable findings whose file sets overlap share one task; the rest get one task each. Findings named in a selected finding's `depends_on` become `needs` between their tasks.
- **Fix task rows.**

| Field | From |
|---|---|
| `title` | the finding's `summary`; for a folded task, the first finding's summary and the count of the rest |
| `files` | each finding's edit target and `instances` |
| `effort` | `trivial` and `small` to `S`, `medium` to `M`; a folded task takes its largest |
| `action`, `detail` | the text `export-v1` composes for the v1 description |
| `spec.outcome` | per finding: the failure scenario no longer happens (defect), or the summary's change is made (cleanup) |
| `spec.premises` | per finding, `tomlctl review anchor <id> <store flag>` at `base` |
| `spec.accept` | cleanup `remove`: `tomlctl review anchor <id> <store flag> --gone`, `forward`, `writes_build = false`. Defect, and cleanup `change` or `add`: `tomlctl review reproduce-status <id> <store flag> --expect gone`, `forward`, left to the gate |
| `spec.constraints` | "Touch no file outside this task's files." and "Change no behaviour the finding does not name."; a finding's `decision_answer` adds "Decision: <answer>" |
| `spec.executor` | `lite` when the task holds at most two files and every finding is a cleanup or a defect of effort `trivial` or `small`; otherwise `deep` |
| `fixes`, `review_store` | the finding ids and the store path |

- **Settle task rows.** One per finding: `files` is `docs/flows/<flow>/notes/settle/<finding id>.md`, marked `(new)`; `executor` is `deep`; `outcome` is that the note opens `Proposal:` or `Refuted:` and answers the finding's `blocked_missing`; the accept check greps the note's first line for either word, `forward`, `writes_build = false`; `settles` names the finding.
- **Audit.** On an approved flow every minted row carries the `reason` `apply <store path>: <finding ids>`.
- **Order of writes.** The task store is written before the review store. A rerun finds the task that already fixes an `open` finding through `[[finding_links]]` and marks the finding without minting a second task, but only when that task row has no `commit`: a link to a committed task is an earlier fix of a finding since reopened, and the finding gets a new task.
- Output lists `scheduled` (finding, task id, ref), `settle`, `held` and `refused`, each with a reason.

## review reconcile

`tomlctl review reconcile <store flag> --flow <slug>` takes the store flag like every review verb, and `--flow` names the apply flow. It starts from the store's own findings: each `scheduled` finding whose `apply_flow` is that slug is looked up in the flow's task store by its `apply_task` ref, and moved by the transition table in [stores.md](stores.md#finding-status-and-link). When the ref names no row, the finding's id is looked for in the flow's `[[finding_links]]`: a link that still names it is a task whose title, and so ref, changed, and reconcile follows it and rewrites `apply_task`. Only a finding no link names has lost its task. Output lists `resolved`, `reopened` and `in_flight`.

Whether the task fixes or settles the finding is read from the flow's `[[finding_links]]`. A finding linked through `settles` is not resolved by its settle task: when that task is `done` with a `commit` and the finding is still `scheduled`, reconcile returns it to `open`.

## review settle

`tomlctl review settle <id> --accept --tier <tier> --fix-file <path> | --refute --citation <text> | --defer --proposal <path>`; see [stores.md](stores.md#proposals). It refuses a finding that is not blocked on `evidence`.

## review triage --answer-file

`tomlctl review triage <id> --answer-file <path> --expect-status open` records a decision's answer; see [stores.md](stores.md#unblocking). It refuses a finding that is not blocked on `decision`.

## review list --blocked-on

`--blocked-on <value>|any|none` replaces `--fix-shape`. `none` lists fixable findings.

## review stats --apply

One object for the store: per lens and per agent, the findings written, the blocked ones by `blocked_on`, and those a verifier unblocked; settle outcomes by tier (`mechanical`, `cited`, `user`), refuted and deferred; apply outcomes per lens and kind (`scheduled`; `resolved` with an apply link; reopened, counted as `open` findings whose `reopen_reason` opens `apply: `); and reproductions by verdict.

## Capabilities

| Milestone | Version | Features added |
|---|---|---|
| A | 0.23.0 | `tasks_finding_links`, `review_schedule`, `review_reconcile`, `review_anchor`, `review_reproduce`, `review_stats_apply` |
| C | 0.24.0 | `review_blocked_on`, `review_settle`, `review_triage_answer`; `review_list_fix_shape` is removed |
