# tomlctl — review reference

The flag surface of the `tomlctl review` group and the shape of the v2 review store it writes: one store per flow at `.claude/flows/<slug>/review-v2.toml`, or per scope at `.claude/review-v2/<scope>.toml`. How a run is composed, which agent calls which verb and how the orchestrator reads the results are the `review-v2-engine` and `review-v2-method` skills' call, not this one's.

## Contents

- [Shared behaviour](#shared-behaviour)
- [Store shape](#store-shape)
- [Field catalogue](#field-catalogue)
- [Roles](#roles)
- [Routing](#routing)
- [Run lifecycle](#run-lifecycle) — `sync-v1`; the `run` verbs (`start`, `update`, `finish`, `list`, `show`) are [review-runs.md](review-runs.md)
- [Coverage](#coverage) — `survey`, `plan`, `cover`
- [Findings and verdicts](#findings-and-verdicts) — `add`, `verdict`, `resolve`
- [Plan records](#plan-records) — flow mode, `target`, `tier`, `proof`, `apply`
- [Reads](#reads) — `list`, `show`, `digest`, `report`, `stats`
- [Dispositions and export](#dispositions-and-export) — `triage`, `export-v1`
- [Apply through a v2 flow](#apply-through-a-v2-flow) — `schedule`, `settle`, `reconcile`, `anchor`, `reproduce`, `reproduce-status` are [review-apply.md](review-apply.md)
- [Catalogue and schema](#catalogue-and-schema) — `catalogue`, `schema`

## Shared behaviour

- **Store target.** Every verb except `catalogue` and `schema` takes exactly one of `--slug` (the flow store) or `--scope` (the flow-less store); `stats` also accepts `--all`. Both names are validated like a flow slug. The store stamps its own `last_updated`, so no verb takes `--no-stamp`.
- **Run directory.** `.review-v2-runs/<store-key>/U<n>/` at the repo root, where `<store-key>` is the slug or `scope-<scope>`. `run start` creates it with a `payloads/` subdirectory, where agents stage their NDJSON payloads, and writes a `.gitignore` holding `*` at `.review-v2-runs/` when none is there. It and every `--out` must resolve, symlinks followed, under `.review-v2-runs/` or `.claude/`; `run start`, `plan --propose` and `report` take `--allow-outside` to override that.
- **Catalogue drift.** A verb that reads a run's catalogue — `survey`, `plan`, `add`, `run finish` and the others that bind or route against it — reloads it from the roots the run recorded and fails `kind=validation` naming `tree_sha` or `config_sha` when either no longer matches the value `run start` recorded; start a new run. A run recorded without `tree_sha` is not checked.
- **Integrity flags.** The writing verbs (`sync-v1`, `run *`, `survey`, `plan`, `cover`, `add`, `verdict`, `resolve`, `reproduce`, `apply`, `triage`, `settle`, `export-v1`, `schedule`, `reconcile`) carry the write bundle — `--allow-outside`, `--no-write-integrity`, `--verify-integrity`, `--strict-integrity`, `--no-create`. The reading verbs (`run list`, `run show`, `list`, `show`, `anchor`, `reproduce-status`, `digest`, `report`, `stats`, `catalogue`, `schema`) carry `--verify-integrity` and `--strict-read`; `report` also takes `--allow-outside`, for its `--out`. The flag tables below leave both bundles out.
- **Missing store.** `sync-v1`, `stats` and `reconcile` read an absent store as empty and create nothing; `run start` seeds one; every other verb fails `kind=not_found`.
- **Payload sources.** Each `--ndjson` takes `-` for stdin, otherwise a file path; a leading `@` is accepted. Prose flags (`--note`, `--rationale`, `--incomplete`, the triage reasons, `--trigger`) read stdin when given `-`, and each has a `-file` twin that reads a file the Write tool staged. Text is read with `\r\n` as `\n` and trimmed; an empty value is refused.
- **Replays are harmless.** `cover`, `verdict`, `resolve`, `reproduce`, `sync-v1`, `triage`, `settle` and `reconcile` compute their result against the stored rows first and write nothing when it changes nothing, and `plan --ndjson` recognises the wave it already minted, so replaying a call that succeeded reports itself `unchanged` rather than writing a duplicate.
- **Partial batches.** `add`, `plan --ndjson`, `verdict --ndjson`, `reproduce`, `triage`, `export-v1` and `schedule` refuse a failing row or id with its reasons, under `refused`, while the rest land. A call whose every row is refused fails `kind=validation` and writes nothing, as does a refused single `verdict`; `export-v1 --all` and `schedule --all` report their refusals and exit 0.
- **Output.** Writes print a compact `{"ok":true,…}` envelope. `list` and `show` are row reports ([Rows and header](../SKILL.md#rows-and-header-per-command)); `plan --propose` without `--out` prints NDJSON text. Errors are `kind=validation` unless named otherwise.

## Store shape

```toml
schema_version = 1                 # 2 while the store holds a reproduction or a scheduled finding
last_updated = 2026-10-12

[id_high_water]                    # prefix → highest id a removal retired

[[runs]]
id = "U1"
command = "review-v2"              # review-v2 | optimise-v2 | review-plan-v2
family = "code"                    # code | perf | plan
depth = "standard"                 # quick | standard | deep
started = "2026-10-12T10:00:00Z"
base = "e23b918…"                  # --base resolved to a commit sha
head = "4a7c01d…"                  # --head resolved to a commit sha; absent measures to HEAD
scope = ["tomlctl/src/review/**"]
changed_lines = 412
catalogue = { project = ".claude/review-lenses", tree_sha = "…" }  # + builtin when --builtin was passed, project_layer_changed
signals = [ { name = "lang:rust", source = "detector", roots = ["tomlctl"] } ]
waivers = [ { concern = "security", reason = "no input handling in scope" } ]
rechecks = "U0"                    # plan family: the run whose applied edits this run re-checks

[[cells]]
id = "C1"
run = "U1"
wave = 1
slice = ["tomlctl/src/review/add.rs"]
concerns = ["correctness", "type-design"]
agent = "review-finder"            # review-finder | review-finder-cf | review-sweeper | plan-executor
reason = "proposed"
bound = [ { concern = "correctness", hash = "9c1e…", path = "…", methods = ["trace"], v1_category = "quality" } ]
status = "covered"                 # assigned | covered | thin | uncovered
sealed = true
ruled_out = [ { file = "tomlctl/src/review/add.rs", symbol = "prepare", reason = "the early return is deliberate" } ]  # set by cover --ruled-out

[[findings]]
id = "F1"
run = "U1"
cell = "C1"
kind = "defect"                    # defect | cleanup | advisory
lens = "correctness"
route = "verifier"                 # verifier | panel | checker | none
file = "tomlctl/src/review/verdict.rs"
line = 88
status = "open"

[[verdicts]]
finding = "F1"
by = "verifier"                    # verifier | panel | checker | audit | orchestrator | sync
verdict = "confirmed"              # confirmed | plausible | refuted | unsettled
citation = "tomlctl/src/review/verdict.rs:88"

[[reproductions]]
finding = "F1"
flow = "auth-apply"                # the apply flow that ran the fix
milestone = "B"
round = 1                          # positive integer
task = "fix-the-retry-bound"       # the apply task's ref; optional
verdict = "gone"                   # gone | reproduces | unsettled
by = "verifier"                    # verifier | orchestrator
citation = "src/retry.rs:40"
at = 2026-11-12                    # stamped by `reproduce`
```

Ids are minted inside the write lock with the prefixes `U` (runs), `C` (cells) and `F` (findings); verdicts carry no id and are keyed by (finding, `by`, `seat`), and reproductions by (finding, `flow`, `milestone`, `round`, `by`). Rows are written in field-catalogue order with unknown keys last, and an empty string, list or absent number is omitted.

**Schema version.** Every write stamps `schema_version = 2` while the store holds a `[[reproductions]]` row or a `scheduled` finding, and `1` otherwise, so a store that needs neither stays readable by a binary that predates them. A store stamped above `2` is refused.

**Finding statuses:** `unverified`, `superseded`, `open`, `refuted`, `merged`, `parked`, `dismissed`, `deferred`, `backlogged`, `converted`, `resolved`, `exported`, `scheduled`. A status owns its companions: `merged` → `merged_into`, `parked` → `park_reason`, `dismissed` → `dismiss_reason`, `deferred` → `defer_reason` + `defer_trigger`, `backlogged` → `backlog_ref`, `converted` → `converted_to`, `resolved` → `resolution` + `resolved`, `open` → `reopen_reason`; the rest own none. Every status write that changes the status, or supplies companions, clears every other status's companions first. `outcome` (`repeat`, `contested`, `reopened`, `regression`, `suppressed`) is set by `resolve`.

**Link fields.** `exported_as` and `export_ledger` name the v1 row a finding was exported to. They are not companions: they survive the moves `sync-v1` makes from `exported` to `resolved`, `dismissed` or `deferred`. A `triage --reopen` of a finding carrying `exported_as` retires the link: it appends `<export_ledger>#<exported_as>` to the computed list `retired_links` and drops both link fields. `sync-v1` and `stats --outcomes` skip a retired link, and `export-v1` follows it only as described under [Matching](#tomlctl-review-export-v1). On the v1 side, a row's `v2_ref` is a string, or an array once two findings share the row.

**Apply link.** `apply_flow` and `apply_task` name the v2 flow and the task `ref` a `scheduled` finding is held by, and `apply_task_id` that task's numeric id, which a retitle does not change; `schedule` sets them and they are computed, never supplied. They are not companions either: `reconcile`'s move to `resolved` keeps them, and its move back to `open` clears them. `scheduled` owns no companion and, like `superseded`, `merged`, `converted` and `exported`, is never moved by `triage`; only `reconcile` moves it. As a `resolve` target it reads as `open` does.

**Blocked findings.** A defect or cleanup that cannot be fixed as written names what it lacks in `blocked_on` — `decision`, `evidence`, `measurement`, `alternatives` or `design` — with its detail fields ([Field catalogue](#field-catalogue)); one without `blocked_on` is fixable. A legacy row stored with the retired `needs-plan` shape and no `blocked_on` reads as blocked on `design`. A block is answered three ways, each clearing `blocked_on`, its detail fields and any legacy `needs-plan` shape and recording `unblocked_by`: a verifier `verdict --unblocks` (`verifier`, with `fix_sketch` and the block it answered as `unblocked_from`), `triage --answer-file` on a decision block (`user`, with `decision_answer`; `blocked_options` is kept), and `settle --accept` on an evidence block (`settle`, with `fix_sketch` and `settle_tier`). These fields are computed and survive every later status. `schedule`, `export-v1` and `--all` selections refuse a finding that is still blocked.

**Deferred proposals.** `settle --defer` records `proposal` (the note's repo-relative path) and `proposal_at` (UTC datetime) on an open evidence-blocked finding and leaves it blocked. Its age is the number of runs of its family started after `proposal_at` (every run of the family when `proposal_at` is missing or unreadable); at age 3 it is overdue. `digest` lists each deferred proposal of the run's family under `proposals`, and `run finish` refuses, even with `--incomplete`, while one is overdue, until `settle --accept` or `--refute` disposes of it or `triage` moves the finding off `open`.

## Field catalogue

The finding fields an agent supplies, with their caps in characters. Every other field is set by tomlctl, and a row supplying one is refused.

| Kind | Required | Optional |
|---|---|---|
| every kind | `kind`, `lens` (one of the cell's concerns; see the cleanup exception below), `file`, `summary` (≤160) | `symbol`, `same_as`, `depends_on` |
| `defect` | `line` (outside a v2 flow), `severity` (`critical` \| `major` \| `minor`), `failure_scenario` (≤600; aim for 450), `fix` (≤800), `effort`, `counter` (≤300), `grade` (`high` \| `medium` \| `low`), `evidence` (≤5 × 300) | — |
| `cleanup` | `line` (outside a v2 flow), `edit` (≤800), `direction` (`add` \| `remove` \| `change`), `effort`; `old` (≤400) for `change` and `remove`, and for `add` unless `target_file` names a new file; never required on a plan review, for any direction | `target_file`, `instances` (≤50 paths), `counter` |
| `defect` or `cleanup`, blocked | `blocked_missing` (≤400) with any block; `blocked_options` (2–6 × 300, each option naming its consequence) for `decision` and `alternatives`; `blocked_metric` (≤200) and `blocked_command` (≤300) for `measurement`; `blocked_contract` (≤600) for `design` | `blocked_on` |
| `advisory` | `angle` (`risk` \| `challenge` \| `probe` \| `alternative` \| `direction`), `rationale` (≤800), `adoption_cost`; `challenges` when `angle = challenge` | `alternative`, `line` |
| plan family, markdown mode | `plan_section` (≤200) on defect and cleanup | `anchor_old` (≤400), `anchor_new` (≤800) |
| plan family, flow mode | `target` (≤300) on defect and cleanup | `expect`, `new`, `proof` on defect and cleanup; `target` on advisory; `line` |

`review add`, not the catalogue, holds the `line` and `plan_section` requirements, since they depend on the mode, so `review schema` lists neither as required. It also holds the **mechanical-cleanup exception**: a `cleanup` row carrying `old` may name any lens the run's catalogue marks `mode = "mechanical"` for the run's family, from any cell, whether or not the cell covers it; it is routed like any other cleanup. In flow mode `plan_section`, `anchor_old` and `anchor_new` are refused; see [Plan records](#plan-records). `tier`, `applied` and `applying` are computed.

`effort` is `trivial`, `small`, `medium` or `large`. A row still supplying the retired shape field (`edits` or `needs-plan`) is refused, naming `blocked_on` as its replacement. Paths (`file`, `target_file`, `instances`) are repo-relative and `/`-separated, with no `..` and no absolute part.

A cell row takes `slice`, `concerns` (1–3), `agent` and `reason`, plus `question` for a `probe:` concern, `pass` (1 or 2) and `facets` edits (`+id` / `-id`). A verdict row takes `finding`, `by`, `verdict`, `citation` and `rationale` (≤600; aim for 450), plus `seat` (1–3, panel only), `severity` (defects only) and `unblocks` (bool) with `fix_sketch` (≤800), each refused without the other; `unsettled` is the checker's alone. `tomlctl review schema` emits the same rules as JSON Schema.

## Roles

`list` and `show` project every row through one of eight roles, `--for <role>`:

| Role | Sees |
|---|---|
| `finder` | The identity fields and status, for a slice's `PRIOR`. A cell carries its assignment: slice, concerns with bound paths, facet and method paths, `pack`, `baselines`. A `review-finder-cf` cell carries no `pack`; it carries the run's `base` instead, to read the slice as `git diff <base>`, and the run's `head` beside it when the run records one. No other cell carries `head`. A plan-family cell adds `plan_documents`, the run's scope, with the plan first, and in flow mode `flow`, the slug. |
| `verifier` | The claim to refute (`failure_scenario`, `counter`, `evidence`) and the `same_as` target; never `grade` or `fix`; the edit fields only when routed to it. |
| `checker` | The cleanup edit fields, the `lens` and the `same_as` target. |
| `implementer` | The fix, what done means, and the latest citation. |
| `orchestrator` | Status, `blocked_on` with its detail, the latest verdict, and the advisory fields until triaged; `failure_scenario` only when contested; never `evidence`. |
| `audit` | The verifier view plus every verdict and `excerpts`: ±5 lines around the finding's `file:line` and each `path:line` citation. |
| `backlog` | A `backlog add` object (`summary`, `kind`, `area`, `tags`, `evidence`, `context`, `origin`), carrying the finding id as `id`. |
| `report` | What the markdown report renders. |

`verifier` and `audit` rows also carry `ruled_out`, `[{cell, reason}]`: each rule-out another cell of the finding's run recorded at the finding's place (matched as for the digest's `ruled_out_hits`), whatever its wave; the key is absent when there is none.

A flow-mode finding's `target` shows in every role but `checker` and `backlog`; `expect`, `new` and `proof` in `verifier`, `implementer`, `audit` and `report`; `tier`, `applied` and `applying` in `implementer`, `orchestrator`, `audit` and `report`.

`blocked_on`, its detail fields and `decision_answer` show in `verifier`, `implementer`, `orchestrator`, `audit` and `report`; `fix_sketch` in `implementer` and `report`; `settle_tier` in `implementer`, `orchestrator`, `audit` and `report`; `unblocked_by` and `unblocked_from` in `finder`, `orchestrator` and `report`; `proposal` and `proposal_at` in `orchestrator` and `report`.

`finder`, `verifier` and `checker` reports also carry `references`, the absolute paths of the `review-v2-method` worked examples (`finding-kinds`, `verifying`, `plan-records`). A cell takes `finder`, `orchestrator` or `report` only.

## Routing

`add` computes each finding's `route`; agents never choose it.

- `advisory` → `none`.
- `defect` → `verifier`, or `panel` for a `critical` defect in a `deep` run.
- `cleanup` on a `plan`-family run → `verifier`, always: the checker's projection hides a flow-mode row's `target` and edit.
- any other `cleanup` → `verifier` when any edited path (`target_file` or `file`, and every `instances` path) matches the vocabulary's `[routing].executable` globs, or matches its `[routing].tests` globs with `direction = remove`; otherwise `checker`.
- A checker's `unsettled` verdict re-routes the finding to `verifier`.

## Run lifecycle

### `tomlctl review sync-v1`

Reads every v1 row whose `v2_ref` names this store — from each distinct `export_ledger` the findings record, plus the default review and optimise ledger of each family the store holds — and moves the finding to the mapped status with one `by = "sync"` verdict citing `<ledger>#<v1 id>`: `fixed` and `applied` → `resolved`; `wontfix`, `wontapply` and `verified-clean` → `dismissed` (reason `v1: <status>: <prose>`); `deferred` → `deferred`. Only findings at `open`, `exported`, `resolved`, `dismissed` or `deferred` move, and each move replaces the companions the finding carried. A `v2_ref` array links the row to each finding it names. A link the finding has retired is skipped with reason `link retired by reopen`.

A v1 row reopened — `open` with a non-empty `reopen_rationale` — carries the reopen back: a finding at `resolved`, `dismissed` or `deferred` moves to `open` with `reopen_reason = "v1: <rationale>"`, and its sync verdict records `open: <rationale>`. That verdict is how a reopen already taken is recognised, so each rationale moves the finding once, and a later disposition of the user's stands. Prints `{ok, ledgers: [{path, missing}], synced: [{finding, v1, v1_status, status}], skipped, unchanged}`.

```bash
tomlctl review sync-v1 --slug <slug>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |

### The `run` verbs

`run start` (with `--base` and the optional `--head` that bound a run), `run update`, `run finish`, `run list` and the read-back `run show` are documented in [review-runs.md](review-runs.md).

## Coverage

### `tomlctl review survey`

Evaluates every `stack`, `diff` and `health` detector of the merged vocabulary — stack over `git ls-files`, diff over the run's `base..head` patch inside its scope (`base..HEAD` when the run records no `head`; a changed file's bytes are read at `head` when it is set, else from the working tree), health over HEAD or a `git log` window — and records each true signal with `source = "detector"` and its `roots`. A detector that cannot be evaluated is recorded with `source = "unknown"` and its reason. A health detector whose `window` exceeds 36,500 days is recorded under `unknown` with the reason "`window` must be at most 36500 days, got <n>"; a valid window is passed to git as an absolute `--max-age=<epoch secs>` cutoff, with no filter when the cutoff reaches the epoch. A commits-per-file detector (`churn-hotspot`) keeps only scope files in its evidence, though it counts their commits repo-wide, so the proposal's `hotspots` never name a file outside the scope. A rerun replaces both and keeps judged signals. Prints `{ok, run, signals: [{name, kind, roots, evidence?}], unknown: [{name, kind, reason}]}`.

```bash
tomlctl review survey --slug <slug> --run U1
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--run` | run id | The run surveyed. | required |

### `tomlctl review plan`

Two modes, exactly one per call.

`--propose` prints the default coverage map as NDJSON and writes nothing to the store: one cell row per line (`slice`, `concerns`, `agent`, `reason = "proposed"`, `pass` on a repeat), then a last `{"proposal":{…}}` line with `run`, `family`, `depth`, `changed_lines`, `ceiling`, `cells`, `executor_cells`, `agents`, `hotspots`, `unselected` (`[{concern, reason}]`), `unsliced`, `over_cap`, `disabled` and, when the task store is empty, `tasks`. With `--out` the NDJSON goes to that file and stdout carries `{ok, out}` merged with the summary.

The proposal never emits more cells than `ceiling` minus the run's non-executor cells already planned, check cells included. Floor-lens slices merge toward half that budget, counted after repeats, before any later tier is admitted, and a lens that joins an existing cell's bundle adds no cell and is never cut for `ceiling`; a sweeper slice is split at 40 files, each part a cell counted against the ceiling. When the budget still cannot hold the cells, slices keep merging past both targets, down to one whole-scope cell per lens, and each sweeper cell left over 40 files is listed under `over_cap` as `{concerns, slice, files}`. A check cut for the budget appears in `unselected` as `check:<id>` with reason `ceiling`, as a cut lens does under its id.

`--ndjson` appends cells as the next wave, status `assigned`, binding each concern's content hash, path, methods and `v1_category` and attaching facets. It skips a `{"proposal":…}` line, so an edited proposal file is written back as is. When the run's newest wave is unsealed, its cells are all still `assigned`, and they are exactly the incoming rows — compared on `slice` in order, `concerns`, `pass`, `question` and `agent` — the call is a replay: it prints that wave's cells with `unchanged: true` and mints nothing. A partial overlap, or a wave with any cell sealed, mints a new wave. Prints `{ok, run, wave, cells: [{id, agent, concerns, pass, facets}], refused, accounting: {unselected, unsliced, disabled}, unchanged}`.

```bash
tomlctl review plan --slug <slug> --run U1 --propose --accept-signals trust-boundary,hot-path --lens -doc-drift --lens +contrarian --out <run-dir>/proposal.ndjson
tomlctl review plan --slug <slug> --run U1 --ndjson <run-dir>/proposal.ndjson
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--run` | run id | The run planned; a finished run is refused. | required |
| `--propose` | bool | Print the default coverage map. | one mode required |
| `--ndjson` | `-` \| path | Cells to append, one JSON object per line. | one mode required |
| `--accept-signals` | `a,b` | Judged signals accepted for the proposal; each must be a `judged` vocabulary signal. Excludes `--ndjson`. | none |
| `--lens` | `+id` \| `-id` | Force a lens into or out of the proposal. Repeatable; a hyphen-led value is accepted. Excludes `--ndjson`. | none |
| `--out` | path | Write the proposal here instead of stdout. Excludes `--ndjson`. | stdout |

A cell row is refused when a `slice` entry is not repo-relative, its `concerns` do not bind (1–3 enabled lenses of one mode and the run's family, one `probe:<slug>` with a `question`, or `check:<id>` entries), or its `agent` is not the one its concerns run on. The one exception is `review-finder-cf`, the finder run without CLAUDE.md, accepted in place of `review-finder` on a cell whose concerns are exactly `["correctness"]`.

### `tomlctl review cover`

Records a cell's coverage status and note, and its rule-outs when `--ruled-out` is passed, and seals the cell. A cell already sealed with the same status and note succeeds unchanged, unless `--ruled-out` brings a different set, which replaces the stored `ruled_out`; a different status or note is refused, and so is any write to a finished run's cell. Without `--ruled-out` the stored rule-outs are kept. Prints `{ok, cell, status, sealed, unchanged}`, plus `ruled_out` (the entry count) only when the flag is passed.

**Rule-outs.** Each `--ruled-out` entry is one site the cell judged deliberate: `file` (required, repo-relative), a `symbol` or a `line` (a positive integer) or both, and `reason` (required, at most 200 characters); any other key is refused. One refused entry refuses the whole call, and nothing is sealed. An entry whose `file` lies outside the cell's slice is refused unless one of the cell's concerns is a mechanical lens, whose sweep reaches past the slice. In flow mode the flow's `tasks.toml` and `plan.toml` are in reach of every plan cell, whatever its slice. An entry matches a finding by the digest's place rule, which needs a symbol or a line, so an entry with neither is refused.

```bash
tomlctl review cover C3 --slug <slug> --status thin --note-file <run-dir>/payloads/C3-note.txt
tomlctl review cover C4 --slug <slug> --status covered --note-file <run-dir>/payloads/C4-note.txt --ruled-out <run-dir>/payloads/C4-ruled-out.ndjson
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | cell id | The cell sealed. | required |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--status` | `covered` \| `thin` \| `uncovered` | The coverage reached. | required |
| `--note` / `--note-file` | text \| path | The coverage line, at most 600 characters; aim for 450. | required, one of |
| `--ruled-out` | `-` \| path | The cell's rule-outs, one JSON object per line, or a `.toml` record file; replaces the stored set. | none |

## Findings and verdicts

### `tomlctl review add`

Writes one cell's full finding set. Each row is checked against the field catalogue, `lens` against the cell's concerns (except a `cleanup` row with `old` on a mechanical lens, [Field catalogue](#field-catalogue)), `old` against its file (within 20 lines of `line`, `\r\n` read as `\n`; in `target_file` when given and existing), a markdown-mode plan row's `anchor_old` against its `plan_section` (exactly once), and a flow-mode plan row's `target`, `expect` and `proof` against the flow's records ([Plan records](#plan-records)). Outside flow mode a defect or cleanup row without `line`, or a markdown plan row without `plan_section`, is refused. `route`, `anchor` and the anchor candidates are computed; a `same_as` must name a candidate, and a row with candidates but no `same_as` gets them as `related`. The cell's earlier `unverified` rows become `superseded`. Defect and cleanup rows are minted `unverified`; an advisory row `open`, with its `tier` in flow mode, or `merged` with `outcome = "suppressed"` when its `same_as` names a dismissed, deferred, parked, backlogged or converted finding. Prints `{ok, run, cell, findings: [{id, kind, severity, route}], superseded, refused}`.

On a sealed cell `add` prints `{ok: false, sealed: true, cell, status, findings}` with the cell's standing findings and exits 1. A finished run is refused.

**Anchor candidates.** Within the row's family, the findings on the same `(file, symbol)` when `symbol` is set, otherwise in the same file within 10 lines. `lens` and `kind` are not part of the key.

```bash
tomlctl review add --slug <slug> --run U1 --cell C3 --ndjson <run-dir>/payloads/C3.ndjson
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--run` | run id | The run the cell belongs to. | required |
| `--cell` | cell id | The cell written. | required |
| `--ndjson` | `-` \| path | The cell's full finding set, one JSON object per line. | required |

### `tomlctl review verdict`

Upserts a verdict on (finding, `by`, `seat`): an identical replay writes nothing and a changed one moves to the end, where readers take the latest from. A finding with no `same_as` takes its status at once — `confirmed` or `plausible` → `open`, `refuted` → `refuted`, `unsettled` → `unverified`, re-routed to `verifier`; a panel decides once two seats agree (two non-refuted → `open`, two refuted → `refuted`). A finding with a `same_as` stays `unverified` for `resolve`. Only `unverified`, `open` and `refuted` findings take a status from a verdict. A verdict with `unblocks` answers a blocked finding: it clears the block and copies its `fix_sketch` onto the finding with `unblocked_by = "verifier"` and the cleared block as `unblocked_from` ([Blocked findings](#store-shape)). Prints `{ok, verdicts: [{finding, verdict, reroute?, unblocks?, fix_sketch?}], refused, unchanged}`.

```bash
tomlctl review verdict F3 --slug <slug> --verdict confirmed --by verifier --citation tomlctl/src/io.rs:88 --rationale-file <run-dir>/payloads/F3.txt
tomlctl review verdict --slug <slug> --ndjson <run-dir>/payloads/checker-1.ndjson
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | finding id | The finding judged. Excludes `--ndjson`. | required without `--ndjson` |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--verdict` | `confirmed` \| `plausible` \| `refuted` \| `unsettled` | The call. | required without `--ndjson` |
| `--by` | `verifier` \| `panel` \| `checker` \| `audit` \| `orchestrator` \| `sync` | Who made it. | required without `--ndjson` |
| `--seat` | 1–3 | The panel vote; required for `panel`, refused otherwise. | none |
| `--severity` | `critical` \| `major` \| `minor` | A reassessed defect severity. | none |
| `--citation` | text | `file:line` or command output backing the verdict. Optional to the parser; a verdict without one is refused `kind=validation`. | none |
| `--rationale` / `--rationale-file` | text \| path | Why, at most 600 characters (aim for 450); refused when absent, as `--citation` is. | none |
| `--strike-same-as` | bool | Clear the finding's `same_as`, so it resolves on its own verdict. | off |
| `--unblocks` | bool | The verifier's answer to a blocked finding; needs a fix sketch. | off |
| `--fix-sketch` / `--fix-sketch-file` | text \| path | How to fix the unblocked finding, at most 800 characters; `-` reads stdin, and is refused when `--rationale` is also `-`. Only with `--unblocks`. | none |
| `--ndjson` | `-` \| path | A batch of verdict rows, one per line; a row may carry `unblocks` and `fix_sketch`. Excludes every single-verdict flag. | none |

Refused: `unblocks` from anyone but the verifier, on a `refuted` verdict, or on a finding with no block (a replay excepted); a verdict on an advisory or `superseded` finding; `severity` on a non-defect; a checker's `plausible`; a checker's `refuted`, unless a `review-sweeper` cell wrote the row or it is a `cleanup` row whose lens the run's catalogue marks `mode = "mechanical"` (a catalogue that cannot be read refuses it, naming why); `unsettled` from anyone but the checker.

### `tomlctl review resolve`

Resolves every `unverified` finding of the run whose `same_as` has complete verdicts, passing until none resolves, so a same-run target always settles first; a `merged` target is followed through `merged_into`, and a `converted` advisory through `converted_to` to its replacement, which must itself be verified before the row resolves. By the target's status and the row's call:

| Target status | `confirmed` | `plausible` | `refuted` |
|---|---|---|---|
| `open`, `exported`, `scheduled` | repeat | repeat | contested |
| `refuted` | reopened | contested | repeat |
| `resolved` | regression | contested | repeat |
| `dismissed`, `deferred`, `parked`, `backlogged` | suppressed | suppressed | suppressed |

A repeat appends the run to the target's `seen_in` and refreshes its `line` and `evidence`; a contested or reopened target gains the deciding verdicts, and a reopened one returns to `open`. The row becomes `merged` with `merged_into`, except a regression, which stays `open` with `related = [target]`. Prints `{ok, linked: [{finding, same_as}], resolved: [{finding, outcome, target, status}], pending: [{finding, same_as, reason}], unchanged}`; `linked` is empty without `--same-as`.

**Linking duplicates.** Each `--same-as <DUP>=<TARGET>` sets the duplicate's `same_as` before the pass, and the pair resolves by the same table whatever wave the duplicate came from, so a `duplicate_candidates` pair whose duplicate has its verdicts settles in one call; one still awaiting them is listed under `pending`. Every pair is checked before any is set, and one refused pair refuses the call with each reason and writes nothing: a finding linked to itself; one duplicate given two targets; an unknown id; either finding from another run, or `superseded`; a pair of different families; a duplicate already `merged` into another row; a duplicate already naming another target in `same_as` (strike it with `verdict --strike-same-as` first); a duplicate at any status but `unverified`, `open` or `refuted`; and a link that would close a cycle through `same_as`, `merged_into` or `converted_to`. A duplicate already merged into, or already naming, its target is accepted and changes nothing.

```bash
tomlctl review resolve --slug <slug> --run U1 --wave 2
tomlctl review resolve --slug <slug> --run U1 --same-as F12=F7 --same-as F15=F9
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--run` | run id | The run resolved. | required |
| `--wave` | N | Only findings from cells of this wave; a `--same-as` duplicate resolves whatever its wave. | every wave |
| `--same-as` | `DUP=TARGET` | Link the duplicate to the target and resolve the pair. Repeatable. | none |

## Plan records

A plan-family run is in **flow mode** when its store is a flow's (`--slug`) and that flow has `.claude/flows/<slug>/plan.toml`; otherwise it is in **markdown mode**, which behaves as the rest of this reference describes. The reviewer-side how-to — what to read, which record each markdown section maps to, worked payloads — is `review-v2-method`'s `references/plan-records.md`; this section is the tool's side.

- **`target`** names one record: `task:<ref>.<field>`, `task:<ref>.spec.<field>`, `task:<ref>.spec.<list>[<n>]` or `[+]`, `decision:D<n>.<key>`, `contract:C<n>.<key>`, `milestone:<id>.<field>`, `handover:H<n>.<field>`, `brief#<Section>` or `design/<file>.md#<anchor>`. `file` and `symbol` must be the target's own, and `line` is optional.
- **`expect` and `new`** are JSON values, stored as JSON text since TOML holds no null. `expect` must equal the record's current value — for a list entry, exactly one entry must equal it, with an accept entry's tool-written `probe` removed on both sides; for prose, it is text occurring exactly once in the section. `new` needs `expect`, except on a `[+]` append; a `new` of `null` removes a list entry. Neither may carry `probe`. An advisory row may carry `target` alone.

**Tiers.** `tier` is computed when a flow-mode finding first opens — by `verdict`, `resolve`, a `triage --reopen`, or `add` for an advisory — and frozen after. The first that holds wins:

1. `question` or `user`: the target is any `decision:*`, any `task:*.spec.outcome`, or `brief#Intent`, `brief#Non-goals`, `brief#Scope budget` or `brief#Success criteria`. Kind `advisory` or no `new` is `question`, which `apply` and `triage --park` refuse and the user closes with `triage --resolve`; a defect or cleanup with `new` is `user`.
2. `advisory`: kind `advisory`, a `handover:*` target, or no `new`.
3. `mechanical`: a `proof` that tomlctl verifies from the store and the tree.
4. `cited`: the latest deciding `verifier` or `panel` verdict is `confirmed` or `plausible`, and its citation is a `path:line` in the target's own file, or in a path the target's value names, with the line inside that file. A `checker` verdict never cites a row into `cited`.

Anything else is `advisory`. Agreement alone never lifts a row past `advisory`. A frozen tier is never migrated: a row stamped `user` before `question` existed stays `user`.

**Proofs.** `proof` is `{kind, …}`, and `add` refuses one that does not hold or a `new` that changes more than it covers. `expect` and `new` are capped at 800 serialised characters each on a proven row.

| `kind` | Holds when | `new` may |
|---|---|---|
| `probe` | the target is a `spec.accept` entry whose recorded probe verdict is not healthy; no verdict proves nothing | change only the entry's `check` and `positive` |
| `check-class` (`class`) | `plan check` reports that class at error severity on the target's task; `task:*` targets only | change only the field the class binds (`plan-records.md` lists them). On a list it may only append entries or fill an existing entry's empty `check`; changing or dropping a stored entry is refused |
| `path-missing` (`path`) | the target is `task:<ref>.files`, the path is absent from the tree and not a `(new)` file note | drop the path or replace it with one other |
| `ref-unknown` (`ref`) | the target is `needs` or `contract:*.tasks` and the value names no task | drop the id or replace it with one other |

### `tomlctl review apply`

Writes the record edits of open flow-mode defect and cleanup findings. Before any write it refuses, as one `kind=validation` error listing each refused id with its reasons: a finding of another family, kind or status than an open plan defect or cleanup; no `tier`, or `advisory`; `question` tier, naming `triage --resolve` as its exit; `user` tier without `--user-accepted`; any finding without `--user-accepted` once `plan.toml` holds an `[[approval]]`; no `target` or no `new`; a `handover:*` target; a `new` the target store's validator rejects; and a mechanical proof that no longer holds. `--scope`, and a flow without `plan.toml`, are refused outright.

- **Writes.** Each target is written through its own store's path with `expect` re-checked inside that store's lock: task rows and specs through the task update path (actor `review`, reason `review <run> <id>`; a `[[changes]]` row only when the store's `audit` is on), decisions and contracts through the plan records path, milestones through the milestone path. A brief or design edit replaces `expect`'s one occurrence in the section, under an exclusive lock and an atomic write, keeping the file's line endings; a non-UTF-8 file is refused.
- **Order.** Each finding is stamped straight after its own write, so a failure leaves at most one edit unstamped. The error reads "applying <id> failed; stamped before it: [ids]. <id>'s edit may or may not have landed: re-run `review apply` with only the ids not listed as stamped, and an edit that landed reads as already applied".
- **Already made.** A value already equal to `new` — for `[+]`, an equal entry exists; for a removal, no entry equals `expect` — is reported under `already`, written nothing, and stamped with `recovered = true`. This is decided before a mechanical proof is re-checked, since a made fix no longer satisfies its proof. A brief or design edit reads as already made only when `new` occurs once in the section with every occurrence of `expect` inside it and either `expect` is part of `new` or the finding carries `applying = true`; a deletion (empty `new`) with `expect` gone needs the mark. Without either, a `new` found with `expect` gone may be chance, and the finding is stale.
- **Write-ahead mark.** Before a brief or design write, the finding gets `applying = true` in the review store. The stamp drops it, and so does a write that finds the edit stale; it stays when the write fails or lands without its stamp, so a re-run recovers an edit that landed. The field is absent otherwise, and record targets never carry it: their compare-and-set judges the whole value or entry, which a chance match cannot satisfy.
- **Stale.** Any other mismatch lands under `skipped_stale` as `{id, target, expected, found}`, and the rest proceed. Under `--dry-run`, a finding whose record an earlier finding in the same call already edits (same file, symbol and field; for list entries and prose, also the same `expect`; appends never collide) lands there too, as `{id, target, expected, found: null, edited_by: <earlier id>}` instead of `applied`. Real runs are unchanged.
- **Stamp.** Status `resolved`, `resolution = "applied to <target>"`, and `applied = {at, tier, target, old, new, by, recovered}`, where `old` and `new` are JSON text and `by` is `tool`, or `user` under `--user-accepted`. Before approval no `[[changes]]` row exists, so `applied` is the edit's record.

Prints `{ok, applied: [{id, target, tier}], already: [ids], skipped_stale}`; `--dry-run` adds `dry_run: true` and writes nothing.

```bash
tomlctl review apply F3,F7 --slug <slug> --dry-run
tomlctl review apply F9 --slug <slug> --user-accepted
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids | Finding ids, comma- or space-separated. | required |
| `--slug` / `--scope` | name | The flow store; `--scope` is refused. | required |
| `--user-accepted` | bool | The user accepted these edits: admits `user` tier and an approved plan, and records `by = "user"`. | off |
| `--dry-run` | bool | Report the outcome without writing. | off |

## Reads

### `tomlctl review list`

Lists findings through a role projection. Prints `{for, count, references?, findings: [rows]}`; `findings` holds the rows the global output options act on.

```bash
tomlctl review list --slug <slug> --in tomlctl/src/review --for finder
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--kind` | `defect` \| `cleanup` \| `advisory` | Keep this kind. | any |
| `--lens` | lens id or `probe:<slug>` | Keep this lens. | any |
| `--run` | run id | Keep findings minted by this run. | any |
| `--cell` | cell id | Keep findings of this cell. | any |
| `--status` | status | Keep this status; `superseded` rows appear only when asked for. | any but `superseded` |
| `--route` | `verifier` \| `panel` \| `checker` \| `none` | Keep this route. | any |
| `--blocked-on` | `decision` \| `evidence` \| `measurement` \| `alternatives` \| `design` \| `any` \| `none` | Keep findings blocked on this; `any` keeps every blocked finding, `none` fixable and advisory ones. A legacy `needs-plan` row counts as `design`; any other value fails `kind=validation`. | any |
| `--in` | glob | Keep findings whose `file` matches, as a `:(glob)` pathspec (`*` within one segment); a plain path also matches everything under it. | any |
| `--for` | role | The projection; see [Roles](#roles). | `orchestrator` |

### `tomlctl review show`

Prints findings (`F<n>`) or cells (`C<n>`) through a role projection, in the order given, as `{for, references?, items: [rows]}`. An unknown id fails `kind=not_found`.

```bash
tomlctl review show F3,F7 --slug <slug> --for audit
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids | Finding or cell ids, comma- or space-separated. | required |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--for` | role | The projection. A cell takes `finder`, `orchestrator` or `report`. | required |

### `tomlctl review digest`

The orchestrator's view of one run, as a single object: `run` (and `since_wave`); `counts` by `kind`, `lens` and `status`; `cells` (`status` counts, `unsealed` ids, the `notes` of `thin` and `uncovered` cells); `unverified` (`[{id, kind, route, cell}]`); the coverage accounting `unselected`, `unsliced` and `disabled`, the first two needing a waiver before `run finish`; `contested` rows with their `target`; `refuted_criticals`; `duplicate_candidates`; `ruled_out_hits`; `audit_sample`; the open `advisory` rows; and `proposals`, `[{id, kind, summary, proposal, proposal_at, age, overdue}]`, every deferred proposal of the run's family in the store, whichever run found it ([Deferred proposals](#store-shape)). Rows are projected `--for orchestrator`, so the digest never carries `evidence`.

- **Place rule.** Two places match when they share a `file` and either both carry a `symbol` and the symbols are equal, or otherwise both carry a `line` within 10 lines of each other. `lens` and `kind` play no part.
- **`duplicate_candidates`**, `[{a, b, file, symbol, cells}]`: pairs of the run's non-superseded findings from different cells whose places match, `a` the lower id, `symbol` the first non-empty of the two, `cells` the two cells in pair order. A pair is omitted when the `same_as` / `merged_into` chains of its members meet. A candidate is a question for `resolve --same-as`, never a fold.
- **`ruled_out_hits`**, `[{finding, cell, ruled_out_by, reason}]`: each finding whose place matches a rule-out another cell of the run recorded with `cover --ruled-out`, once per matching entry. A cell's own rule-outs never count against its findings.
- **`audit_sample`**: `confirmed_defects` (standing defects, whose latest non-audit verdict is `confirmed` or `plausible`), `refuted_defects` (refuted non-critical defects) and `checked_cleanup` (checker-decided cleanup, including a sweeper row when there is one), two each, picked by position in store order. Each `refuted_defects` slot left empty takes one more standing defect, so `confirmed_defects` holds up to four.

Under `--since-wave`, a duplicate pair shows when either member is in the window, and rule-outs from every wave count against the windowed findings.

```bash
tomlctl review digest --slug <slug> --run U1 --since-wave 2
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--run` | run id | The run summarised; an unknown run is `kind=not_found`. | required |
| `--since-wave` | N | Only cells and findings from this wave onward. | every wave |

### `tomlctl review report`

Writes the run's markdown report — unblocked defects by severity, a Blocked group with one subsection per `blocked_on` value holding its defects and cleanup with what each lacks (missing, options, metric, command, contract), unblocked cleanup by file, advisory with dispositions, the coverage map — from the store alone, so one store always writes the same bytes. The report is derived and has no sidecar. Prints `{path, counts}`, where `counts` is the report's counts line (`defect n · cleanup n · advisory n · refuted n · suppressed n · unverified n · cells n · uncovered n`).

```bash
tomlctl review report --slug <slug> --run U1 --out <run-dir>/report.md
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--run` | run id | The run reported. | required |
| `--out` | path | Where the report is written; under `.review-v2-runs/` or `.claude/`, like every `--out`. | required |
| `--allow-outside` | bool | Write `--out` outside those roots. | off |

### `tomlctl review stats`

Rates per lens, probe, run or model over one store or all of them. Writes nothing. Prints `{ok, stores: [paths], by?, rows?, outcomes?, apply?, escapes?, calibration?, plan_outcomes?}`.

- **Rows** (`--by`, or no section flag): `key`, `families`, `runs`, `cells`, `findings` by kind, `yield_per_cell`, `verified`, `refuted`, `refute_rate`, `audited`, `overturned`, `audit_overturn_rate`, `dismissed`, `dismissal_rate`, `thin`, `uncovered`, `uncovered_rate`; per lens also `hashes`, `waived` and `waiver_rate`, per probe tag `probes`. A rate over zero is `null`. A probe groups under the tag its `reason` opens with (`nearest <tag>:`), else `untagged`.
- **`--by run`** rows also carry `store`, `command`, `depth`, `started`, `finished`, `waves` (derived from the run's cells, as `run list` does), `agents` and `waivers` (`concerns`, `files`).
- **`--by model`** rows are keyed by the model of the verifying role that decided each finding — `models.verifier` for `verifier` and `panel`, `models.checker` for `checker`, read from the run that verdict names — and carry `roles`, `runs`, `verified`, `refuted`, `refute_rate`, `audited`, `overturned` and `audit_overturn_rate`. A run without `models`, or without the role, keys as `unknown`.
- **`plan_outcomes`** (`--plan-outcomes`; `--slug` or `--all`, flow stores only, plan family only): per run and in `total`, `tiers` (tier → disposition → count; dispositions `applied`, `applied-disclosed` for cited, `user-accepted`, `user-vetoed` for a dismissed `user` row or a dismissed non-advisory tiered row of a run started at or after the plan's first `[[approval]]`, `user-answered` for a `question` row resolved without an edit, else the status), `veto_rate` (vetoed over accepted plus vetoed; `user-answered` counts on neither side), `growth` (`edits`, net `chars_added`, `entries_added`, `entries_removed`), `recheck_yield` (findings minted by a run with `rechecks`), and `cost` (`agents`, `minutes`); each run also names `rechecks`. A rate or yield with nothing to count is `null`.
- **`outcomes`**: per lens and kind, `exported`, `linked`, the v1 statuses, the mapped v2 statuses and `wontfix_reasons`, through `v2_ref` with `sync-v1`'s mapping; a retired link is not counted.
- **`apply`** (`--apply`): the native counterpart of `outcomes`, read from the store alone with no v1 ledger opened. `outcomes` holds one row per lens and kind with any natively applied finding — `{lens, kind, scheduled, resolved, reopened, reproductions}` — where `scheduled` counts findings at that status, `resolved` resolved findings that keep an `apply_flow`, `reopened` open findings whose `reopen_reason` starts `apply: `, and `reproductions` maps each verdict (`gone`, `reproduces`, `unsettled`) to the findings whose current reproduction has it. `reproductions` is `{rows, findings}`, each a verdict → count map: `rows` over every reproduction row, `findings` over each finding's current one. A reproduction of a finding `--family` drops is not counted. `blocked` is `{lens, agent}`, one row per lens and per agent of the finding's cell (`unknown` when the cell names none): `key`, `findings` (every kind), `blocked`, `blocked_rate`, `blocked_on` (each block; an answered block is read from `unblocked_from`, else from `unblocked_by`, and a verifier unblock stored without `unblocked_from` counts under `unrecorded`), `verifier_unblocked`, `verifier_unblock_rate` and the row's `settle` counts. `settle` counts settle outcomes over the view: `mechanical`, `cited` and `user` accepts by tier, `refuted` and `deferred`. A high `mechanical` count or `verifier_unblock_rate` reads as findings blocked that could have been fixed as written.
- **`escapes`**: `count`, `by_concern` and `findings` minted in a file and concern an earlier run's cell had sealed `covered`.
- **`calibration`**: per area, the top exemplar sentences with `count` and `sources`, drawn only from user dismissals, v1 wontfix reasons and audit overturns.

```bash
tomlctl review stats --all --by lens
tomlctl review stats --slug <slug> --outcomes --escapes --calibration --exemplars 3
tomlctl review stats --all --apply
tomlctl review stats --all --plan-outcomes --by model
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` / `--all` | name \| bool | One store, or every flow and flow-less store. | required, one of |
| `--family` | `code` \| `perf` \| `plan` | Only this family. | every family |
| `--by` | `lens` \| `probe` \| `run` \| `model` | The grouping axis of `rows`. | `lens` |
| `--plan-outcomes` | bool | Add the plan-review outcomes; refused with `--scope`. | off |
| `--outcomes` | bool | Add the v1 apply outcomes. | off |
| `--apply` | bool | Add the native apply outcomes, reproductions, blocked rates and settle outcomes. | off |
| `--escapes` | bool | Add the coverage escapes. | off |
| `--calibration` | bool | Add the calibration exemplars. | off |
| `--exemplars` | N | Exemplars per area; needs `--calibration`. | 5 |

## Dispositions and export

### `tomlctl review triage`

Applies one disposition to each id. A row already in the requested end state is `unchanged`; under `--expect-status`, an id no longer at that status lands under `skipped_stale` as `{id, field, expected, found}`, checked inside the locked write. A move clears every earlier disposition's companions. A `--reopen` also retires the finding's v1 link (see [Link fields](#store-shape)), so the next `sync-v1` does not undo it and a re-export mints a regression row. A `superseded`, `merged`, `converted`, `exported` or `scheduled` finding is never moved. Prints `{ok, mode, applied: [{id, from, status}], unchanged, skipped_stale, refused, minted?}`.

```bash
tomlctl review triage F3,F9 --slug <slug> --dismiss-file <run-dir>/payloads/dismiss.txt --expect-status open
tomlctl review triage F4 --slug <slug> --defer-file <run-dir>/payloads/defer.txt --trigger-file <run-dir>/payloads/trigger.txt
tomlctl review triage F8 --slug <slug> --convert cleanup --ndjson <run-dir>/payloads/F8-convert.json
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids | Finding ids, comma- or space-separated. | required |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--dismiss` / `--dismiss-file` | text \| path | Status → `dismissed`, recording `dismiss_reason`. | one disposition required |
| `--defer` / `--defer-file` | text \| path | Status → `deferred`, recording `defer_reason`; needs a trigger. | one disposition required |
| `--trigger` / `--trigger-file` | text \| path | What reopens a deferred finding; with `--defer` only. | none |
| `--reopen` / `--reopen-file` | text \| path | Status → `open` from `dismissed`, `deferred`, `parked`, `backlogged`, `refuted` or `resolved`. | one disposition required |
| `--park` / `--park-file` | text \| path | Status → `parked`; advisory only, and never a `user` or `question` tier row. | one disposition required |
| `--resolve` / `--resolve-file` | text \| path | Status → `resolved`, recording `resolution` and today's `resolved`. | one disposition required |
| `--backlog-ref` | `B-<hex>` | Status → `backlogged`, recording the backlog item id. | one disposition required |
| `--convert` | `cleanup` \| `defect` | Mint the advisory's replacement from `--ndjson`; status → `converted`. Exactly one advisory id. | one disposition required |
| `--answer-file` | path | The user's answer to an open finding blocked on `decision`, at most 800 characters; the status stays `open`. | one disposition required |
| `--ndjson` | `-` \| path | The replacement as one JSON object; with `--convert` only. | none |
| `--run` | run id | The open run the replacement is minted in when the advisory's run is finished; with `--convert` only. | the advisory's run |
| `--expect-status` | status | Move only the ids still at this status. | no check |

`--convert` defaults the replacement's lens, anchor, summary and, in flow mode, `target` from the advisory, validates and routes it, records an `orchestrator` verdict on it and leaves it `unverified` for its route; `minted` reports `{id, kind, severity, route, status}`, and a replay reports the replacement already minted.

`--convert` accepts a `question` tier advisory, though `--park` refuses one: it is how a question becomes a proposed edit. The replacement targets the same user-owned record, so once verified it is tiered `user` when it carries a `new`, and `question` again when it does not. A `user` tier replacement still applies only through `review apply --user-accepted`, so the conversion never makes the edit without the user's Accept.

The replacement is minted in an open run, which cannot finish until it is verified. While the advisory's run is open it is that run, and a `--run` naming any other is refused. Once the advisory's run is finished, the call is refused unless `--run` names an open run of the advisory's family; an unknown, finished or other-family run is refused too. The replacement then takes the named run as `run` and `seen_in`, keeps the advisory's `cell` as provenance, is routed by the named run's catalogue and depth, and its `orchestrator` verdict is filed under that run.

`--answer-file` (mode `answer`) records the file's text as `decision_answer` with `unblocked_by = "user"` and clears the block, keeping `blocked_options` as what the answer chose among; `schedule` binds the answer to the finding's fix task as a constraint. A finding not `open`, or blocked on anything but `decision`, is refused, and a replay of the same answer is `unchanged`.

### `tomlctl review export-v1`

Exports open defect and cleanup findings into a v1 ledger for `/review-apply` or `/optimise-apply`. The ledger's family comes from `--to`: `review-ledger.toml` or a path under `.claude/reviews/` takes `code` findings with `R` ids; `optimise-findings.toml` or a path under `.claude/optimise-findings/` takes `perf` findings with `O` ids.

- **Refused**, with a reason: the plan family, advisory, any status but `open`, another family than the ledger's, a blocked finding ([Blocked findings](#store-shape)), `effort = large`.
- **`--all`** takes every open defect and cleanup finding of the ledger's family in this store — there is no run filter — and holds back, under `held`, test removals and cleanup whose latest verdict is `plausible`; those export only by explicit id.
- **Rows.** A defect exports with its latest severity (`major` → `warning`, `minor` → `suggestion`), cleanup as `suggestion`; `category` is the bound lens's `v1_category`, or `quality` / `algorithm` for a probe or check cell; `description` is built from the implementer projection. The v1 row carries `v2_ref = "<store path>#F<n>"`.
- **Matching.** A v1 row whose `v2_ref` names the finding — the string, or an entry of the array — is `linked`. Otherwise an open row is `reused`, its `rounds` bumped, only on the strict fold: its `v2_ref` names the finding this finding's `same_as` names, or its `file`, `symbol` and `summary` all equal the finding's and its `v2_ref` names a finding of this store with the same `lens`. A v1 row carries `category`, never `lens`, so a row without a resolvable `v2_ref` never meets the second clause, and that clause never matches a row this same call wrote. Failing that, the loose v1 merge rule (same `file`, and the same non-empty `symbol` or the exact `summary`) is read for settled rows only: a deferred, wontfix, wontapply or verified-clean match is skipped, and a fixed or applied match gets a new id with `related` (`regression`). Anything else — an open row that matches only loosely included — is `minted`. A reused row whose `v2_ref` names another finding becomes an array naming both, rather than being overwritten.
- **Retired links.** A row the finding retired on reopen is never `linked` and never skipped as settled; it joins the merge rule as a candidate. Open again in v1, it is `reused` and the link is restored, its `retired_links` entry dropped; fixed, applied or settled, it yields a `regression` row with `related` naming it.
- **Writes.** The v1 ledger is written and committed first; then each finding becomes `exported` with `exported_as` and `export_ledger`. A new ledger is seeded, and the ledger's `last_updated` becomes the later of its own and the newest run date among the findings.

Prints `{ok, ledger, exported: [{finding, v1, action}], skipped, held, refused, dropped_deps, skipped_stale}`; under `--dry-run` the same fields sit under `would_change` beside `dry_run: true`, and nothing is written. `dropped_deps` lists `depends_on` ids that map to no exported v1 row.

```bash
tomlctl review export-v1 F1,F4 --scope <scope> --to .claude/reviews/<scope>.toml
tomlctl review export-v1 --all --slug <slug> --to .claude/flows/<slug>/review-ledger.toml --dry-run
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids | Finding ids, comma- or space-separated. Excludes `--all`. | required without `--all` |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--all` | bool | Every exportable finding of the ledger's family in this store. | off |
| `--to` | path | The v1 ledger written. | required |
| `--dry-run` | bool | Report `would_change` without writing either file. | off |

## Apply through a v2 flow

`schedule`, `settle`, `reconcile`, `anchor`, `reproduce` and `reproduce-status` — fixing code and perf findings through a v2 flow's task store, settling evidence-blocked findings, and the build-free checks a fix task asks — are documented in [review-apply.md](review-apply.md).

## Catalogue and schema

### `tomlctl review catalogue`

Merges the built-in and project layers and applies the project config. Takes no store. Prints `{tree_sha, config_sha, entries}`, one entry per lens, facet, method and check: `type`, `id`, `hash`, `status`, `families`, `mode`, `produces`, `activation`, `apply_when` (`signals`, `paths`, `min_depth`, `always_at`), `matched`, `attaches_to`, `executor`, `command`, `path`, `layer`, `disabled`. `executor` (`tomlctl`, `surveyor`, `verification`, `cell`) and `command` belong to checks, `null` on the other types; a project check from `.claude/review-lenses/config.toml` prints `executor = "verification"` and its configured `command`. Every mode but `--check` fails when a catalogue file could not be read or parsed, listing those errors; `--check` reports them with the rest.

```bash
tomlctl review catalogue --check
tomlctl review catalogue --family code --signals lang:rust,public-api-change
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--builtin` | dir | The built-in catalogue root. | `<claude dir>/skills/review-v2-lenses` |
| `--project` | dir | The project layer; a named directory must exist (`kind=not_found`), as for `run start`. | `.claude/review-lenses` under the repo root, when present |
| `--family` | `code` \| `perf` \| `plan` | Only entries of this family. | every family |
| `--signals` | `a,b` | Mark the entries these signals match, under `matched`; a name the vocabulary does not declare is refused. | none |
| `--vocabulary` | bool | Print `{signals: [{name, kind, meaning, detect, hint, layer}]}` instead. | off |
| `--owners` | bool | Print `{owners: [{family, tag, owners}]}`, the concern-ownership map, instead. | off |
| `--check` | bool | Validate; print `{"ok":true}`, or exit 1 listing every error. | off |

`--check` covers unique ids, the frontmatter schema per type, signals and tags drawn from the merged vocabulary, one owner per (family, tag) matching each lens's Owns section, existing `methods` and `attaches_to` targets, a Probes section per declared family, config and scope ids that exist, no id both enabled and disabled, the `project:` prefix rule for project additions, every `contains` pattern compiling, and `v1_category` valid per family. `run start` runs the same validation.

### `tomlctl review schema`

Prints a JSON Schema (draft 2020-12) built from the field catalogue. `finding` is the row `review add` accepts — closed, with each kind's required set; `cell-return` is what a cell agent returns, `{cell, findings: [{id, kind, severity, route}], status, escalate?}` — `add`'s cell and findings with the status `cover` sealed; `verdict-return` is `{finding, verdict, reroute?, unblocks?, fix_sketch?}`, each of the last two requiring the other.

```bash
tomlctl review schema finding --kind cleanup
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | `finding` \| `cell-return` \| `verdict-return` | The schema printed. | required |
| `--kind` | `defect` \| `cleanup` \| `advisory` | Narrow `finding` to one kind; refused on the other targets. | every kind |
