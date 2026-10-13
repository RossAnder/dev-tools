# tomlctl — review reference

The flag surface of the `tomlctl review` group and the shape of the v2 review store it writes: one store per flow at `.claude/flows/<slug>/review-v2.toml`, or per scope at `.claude/review-v2/<scope>.toml`. How a run is composed, which agent calls which verb and how the orchestrator reads the results are the `review-v2-engine` and `review-v2-method` skills' call, not this one's.

## Contents

- [Shared behaviour](#shared-behaviour)
- [Store shape](#store-shape)
- [Field catalogue](#field-catalogue)
- [Roles](#roles)
- [Routing](#routing)
- [Run lifecycle](#run-lifecycle) — `sync-v1`, `run start`, `run update`, `run finish`
- [Coverage](#coverage) — `survey`, `plan`, `cover`
- [Findings and verdicts](#findings-and-verdicts) — `add`, `verdict`, `resolve`
- [Reads](#reads) — `list`, `show`, `digest`, `report`, `stats`
- [Dispositions and export](#dispositions-and-export) — `triage`, `export-v1`
- [Catalogue and schema](#catalogue-and-schema) — `catalogue`, `schema`

## Shared behaviour

- **Store target.** Every verb except `catalogue` and `schema` takes exactly one of `--slug` (the flow store) or `--scope` (the flow-less store); `stats` also accepts `--all`. Both names are validated like a flow slug. The store stamps its own `last_updated`, so no verb takes `--no-stamp`.
- **Run directory.** `.claude/review-v2-runs/<store-key>/U<n>/`, where `<store-key>` is the slug or `scope-<scope>`. `run start` creates it with a `payloads/` subdirectory, where agents stage their NDJSON payloads.
- **Integrity flags.** The writing verbs (`sync-v1`, `run *`, `survey`, `plan`, `cover`, `add`, `verdict`, `resolve`, `triage`, `export-v1`) carry the write bundle — `--allow-outside`, `--no-write-integrity`, `--verify-integrity`, `--strict-integrity`, `--no-create`. The reading verbs (`list`, `show`, `digest`, `report`, `stats`, `catalogue`, `schema`) carry `--verify-integrity` and `--strict-read`. The flag tables below leave both bundles out.
- **Missing store.** `sync-v1` and `stats` read an absent store as empty and create nothing; `run start` seeds one; every other verb fails `kind=not_found`.
- **Payload sources.** Each `--ndjson` takes `-` for stdin, otherwise a file path; a leading `@` is accepted. Prose flags (`--note`, `--rationale`, `--incomplete`, the triage reasons, `--trigger`) read stdin when given `-`, and each has a `-file` twin that reads a file the Write tool staged. Text is read with `\r\n` as `\n` and trimmed; an empty value is refused.
- **Retries are harmless.** `cover`, `verdict`, `resolve`, `sync-v1` and `triage` compute their result against the stored rows first and write nothing when it changes nothing, so a retry after the Windows sidecar-rename `os error 5` reports itself `unchanged`.
- **Partial batches.** `add`, `plan --ndjson`, `verdict --ndjson`, `triage` and `export-v1` refuse a failing row or id with its reasons, under `refused`, while the rest land. A call whose every row is refused fails `kind=validation` and writes nothing, as does a refused single `verdict`; `export-v1 --all` reports its refusals and exits 0.
- **Output.** Writes print a compact `{"ok":true,…}` envelope. `list` and `show` are row reports ([Rows and header](../SKILL.md#rows-and-header-per-command)); `plan --propose` without `--out` prints NDJSON text. Errors are `kind=validation` unless named otherwise.

## Store shape

```toml
schema_version = 1
last_updated = 2026-10-12

[id_high_water]                    # prefix → highest id a removal retired

[[runs]]
id = "U1"
command = "review-v2"              # review-v2 | optimise-v2 | review-plan-v2
family = "code"                    # code | perf | plan
depth = "standard"                 # quick | standard | deep
started = "2026-10-12T10:00:00Z"
base = "e23b918…"                  # --base resolved to a commit sha
scope = ["tomlctl/src/review/**"]
changed_lines = 412
catalogue = { builtin = "…/skills/review-v2-lenses", project = ".claude/review-lenses", tree_sha = "…" }
signals = [ { name = "lang:rust", source = "detector", roots = ["tomlctl"] } ]
waivers = [ { concern = "security", reason = "no input handling in scope" } ]

[[cells]]
id = "C1"
run = "U1"
wave = 1
slice = ["tomlctl/src/review/add.rs"]
concerns = ["correctness", "type-design"]
agent = "review-finder"            # review-finder | review-sweeper | plan-executor
reason = "proposed"
bound = [ { concern = "correctness", hash = "9c1e…", path = "…", methods = ["trace"], v1_category = "quality" } ]
status = "covered"                 # assigned | covered | thin | uncovered
sealed = true

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
```

Ids are minted inside the write lock with the prefixes `U` (runs), `C` (cells) and `F` (findings); verdicts carry no id and are keyed by (finding, `by`, `seat`). Rows are written in field-catalogue order with unknown keys last, and an empty string, list or absent number is omitted.

**Finding statuses:** `unverified`, `superseded`, `open`, `refuted`, `merged`, `parked`, `dismissed`, `deferred`, `backlogged`, `converted`, `resolved`, `exported`. Each disposition carries its companion: `merged_into`, `park_reason`, `dismiss_reason`, `defer_reason` + `defer_trigger`, `reopen_reason`, `backlog_ref`, `converted_to`, `resolution` + `resolved`, `exported_as` + `export_ledger`. `outcome` (`repeat`, `contested`, `reopened`, `regression`, `suppressed`) is set by `resolve`.

## Field catalogue

The finding fields an agent supplies, with their caps in characters. Every other field is set by tomlctl, and a row supplying one is refused.

| Kind | Required | Optional |
|---|---|---|
| every kind | `kind`, `lens` (one of the cell's concerns), `file`, `summary` (≤160) | `symbol`, `same_as`, `depends_on` |
| `defect` | `line`, `severity` (`critical` \| `major` \| `minor`), `failure_scenario` (≤600), `fix` (≤800), `fix_shape` (`edits` \| `needs-plan`), `effort`, `counter` (≤300), `grade` (`high` \| `medium` \| `low`), `evidence` (≤5 × 300) | — |
| `cleanup` | `line`, `edit` (≤800), `direction` (`add` \| `remove` \| `change`), `effort`; `old` (≤400) for `change` and `remove`, and for `add` unless `target_file` names a new file | `target_file`, `instances` (≤50 paths), `counter` |
| `advisory` | `angle` (`risk` \| `challenge` \| `probe` \| `alternative` \| `direction`), `rationale` (≤800), `adoption_cost`; `challenges` when `angle = challenge` | `alternative`, `line` |
| plan family | `plan_section` (≤200) on defect and cleanup | `anchor_old` (≤400), `anchor_new` (≤800) |

`effort` is `trivial`, `small`, `medium` or `large`. Paths (`file`, `target_file`, `instances`) are repo-relative and `/`-separated, with no `..` and no absolute part.

A cell row takes `slice`, `concerns` (1–3), `agent` and `reason`, plus `question` for a `probe:` concern, `pass` (1 or 2) and `facets` edits (`+id` / `-id`). A verdict row takes `finding`, `by`, `verdict`, `citation` and `rationale` (≤600), plus `seat` (1–3, panel only) and `severity` (defects only); `unsettled` is the checker's alone. `tomlctl review schema` emits the same rules as JSON Schema.

## Roles

`list` and `show` project every row through one of eight roles, `--for <role>`:

| Role | Sees |
|---|---|
| `finder` | The identity fields and status, for a slice's `PRIOR`. A cell carries its assignment: slice, concerns with bound paths, facet and method paths, `pack`, `baselines`. |
| `verifier` | The claim to refute (`failure_scenario`, `counter`, `evidence`) and the `same_as` target; never `grade` or `fix`; the edit fields only when routed to it. |
| `checker` | The cleanup edit fields and the `same_as` target. |
| `implementer` | The fix, what done means, and the latest citation. |
| `orchestrator` | Status, `fix_shape`, the latest verdict, and the advisory fields until triaged; `failure_scenario` only when contested; never `evidence`. |
| `audit` | The verifier view plus every verdict and `excerpts`: ±5 lines around the finding's `file:line` and each `path:line` citation. |
| `backlog` | A `backlog add` object (`summary`, `kind`, `area`, `tags`, `evidence`, `context`, `origin`), carrying the finding id as `id`. |
| `report` | What the markdown report renders. |

`finder`, `verifier` and `checker` reports also carry `references`, the absolute paths of the `review-v2-method` worked examples. A cell takes `finder`, `orchestrator` or `report` only.

## Routing

`add` computes each finding's `route`; agents never choose it.

- `advisory` → `none`.
- `defect` → `verifier`, or `panel` for a `critical` defect in a `deep` run.
- `cleanup` → `verifier` when any edited path (`target_file` or `file`, and every `instances` path) matches the vocabulary's `[routing].executable` globs, or matches its `[routing].tests` globs with `direction = remove`; otherwise `checker`.
- A checker's `unsettled` verdict re-routes the finding to `verifier`.

## Run lifecycle

### `tomlctl review sync-v1`

Reads every v1 row whose `v2_ref` names this store — from each distinct `export_ledger` the findings record, plus the default review and optimise ledger of each family the store holds — and moves the finding to the mapped status with one `by = "sync"` verdict citing `<ledger>#<v1 id>`: `fixed` and `applied` → `resolved`; `wontfix`, `wontapply` and `verified-clean` → `dismissed` (reason `v1: <status>: <prose>`); `deferred` → `deferred`. Only findings at `open`, `exported`, `resolved`, `dismissed` or `deferred` move. Prints `{ok, ledgers: [{path, missing}], synced: [{finding, v1, v1_status, status}], skipped, unchanged}`.

```bash
tomlctl review sync-v1 --slug <slug>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |

### `tomlctl review run start`

Mints `U<n>`, stamps `started`, resolves `--base` to a commit sha, computes `changed_lines` (lines added plus removed over `scope` in `base..HEAD`, or the scope's tracked line count when that is 0; absent for the plan family), validates the merged catalogue — halting with every error — records the catalogue roots and shas, and creates the run directory. Prints `{ok, run, dir, base, changed_lines}`.

```bash
tomlctl review run start --slug <slug> --command review-v2 --family code --depth standard --base HEAD~3 --set-json 'scope=["tomlctl/src/**"]' --set substrate=workflow
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--command` | `review-v2` \| `optimise-v2` \| `review-plan-v2` | The carrier. | required |
| `--family` | `code` \| `perf` \| `plan` | The finding family. | required |
| `--depth` | `quick` \| `standard` \| `deep` | The depth envelope. | required |
| `--base` | rev | The commit the change is measured from; stored as its sha. | required |
| `--builtin` | dir | The built-in catalogue root, recorded as an absolute path. | `<claude dir>/skills/review-v2-lenses` |
| `--project` | dir | The project catalogue layer; a named directory must exist. | `.claude/review-lenses` when present |
| `--set` / `--set-json` / `--set-file` | `KEY=…` | Only `scope` (repo-relative paths or globs), `models` (a table) and `substrate` (`workflow` \| `agent-waves`). | none |

The claude dir is `CLAUDE_CONFIG_DIR`, else `USERPROFILE` or `HOME` joined with `.claude`.

### `tomlctl review run update`

Patches one run. Prints `{ok, run, updated: [keys]}`.

```bash
tomlctl review run update U1 --slug <slug> --set-json 'waivers=[{"concern":"security","reason":"no input handling in scope"}]'
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | run id | The run patched. | required |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--set-json` / `--set` / `--set-file` | `KEY=…` | Only `signals`, `waivers`, `audit`, `agents` and `baselines`. | at least one |

- `signals` are upserted by `name`. They may only add `judged` entries (`source` absent or `judged`; keys `name`, `source`, `roots`, `accepted`, `evidence`) or set an existing judged entry's `accepted`; a detector entry is never changed here.
- `waivers` name exactly one of `concern` and `file`, with a `reason`; a repeated key replaces its reason.
- `agents` is a non-negative integer; `audit` and `baselines` are tables merged into the stored ones. Each `baselines` value is a log path or `none — <reason>`.

### `tomlctl review run finish`

Stamps `finished` and computes `counts` (`cells`, `uncovered`, `defect`, `cleanup`, `advisory`, `refuted`, `suppressed`). Refuses while a finding is `unverified`, a `same_as` awaits `resolve`, or an unselected matched concern or unsliced scope file has no waiver — unless `--incomplete` records why, in which case the reason and the `gaps` are printed and stored. A later clean finish clears an earlier `incomplete`. Prints `{ok, run, finished, counts}`.

```bash
tomlctl review run finish U1 --slug <slug> --incomplete-file <run-dir>/payloads/incomplete.txt
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | run id | The run finished. | required |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--incomplete` / `--incomplete-file` | text \| path | Finish despite open gaps, recording why. | none |

## Coverage

### `tomlctl review survey`

Evaluates every `stack`, `diff` and `health` detector of the merged vocabulary — stack over `git ls-files`, diff over the run's `base..HEAD` patch inside its scope, health over HEAD or a `git log` window — and records each true signal with `source = "detector"` and its `roots`. A detector that cannot be evaluated is recorded with `source = "unknown"` and its reason. A rerun replaces both and keeps judged signals. Prints `{ok, run, signals: [{name, kind, roots, evidence?}], unknown: [{name, kind, reason}]}`.

```bash
tomlctl review survey --slug <slug> --run U1
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--run` | run id | The run surveyed. | required |

### `tomlctl review plan`

Two modes, exactly one per call.

`--propose` prints the default coverage map as NDJSON and writes nothing to the store: one cell row per line (`slice`, `concerns`, `agent`, `reason = "proposed"`, `pass` on a repeat), then a last `{"proposal":{…}}` line with `run`, `family`, `depth`, `changed_lines`, `ceiling`, `cells`, `executor_cells`, `agents`, `hotspots`, `unselected` (`[{concern, reason}]`), `unsliced`, `disabled` and, when the task store is empty, `tasks`. With `--out` the NDJSON goes to that file and stdout carries `{ok, out}` merged with the summary.

`--ndjson` appends cells as the next wave, status `assigned`, binding each concern's content hash, path, methods and `v1_category` and attaching facets. It skips a `{"proposal":…}` line, so an edited proposal file is written back as is. Prints `{ok, run, wave, cells: [{id, agent, concerns, pass, facets}], refused, accounting: {unselected, unsliced, disabled}}`.

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

A cell row is refused when a `slice` entry is not repo-relative, its `concerns` do not bind (1–3 enabled lenses of one mode and the run's family, one `probe:<slug>` with a `question`, or `check:<id>` entries), or its `agent` is not the one its concerns run on.

### `tomlctl review cover`

Records a cell's coverage status and note and seals the cell. A cell already sealed with the same status and note succeeds unchanged; a different one is refused. Prints `{ok, cell, status, sealed, unchanged}`.

```bash
tomlctl review cover C3 --slug <slug> --status thin --note-file <run-dir>/payloads/C3-note.txt
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | cell id | The cell sealed. | required |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--status` | `covered` \| `thin` \| `uncovered` | The coverage reached. | required |
| `--note` / `--note-file` | text \| path | The coverage line, at most 600 characters. | required, one of |

## Findings and verdicts

### `tomlctl review add`

Writes one cell's full finding set. Each row is checked against the field catalogue, `lens` against the cell's concerns, `old` against its file (within 20 lines of `line`, `\r\n` read as `\n`; in `target_file` when given and existing), and a plan row's `anchor_old` against its `plan_section` (exactly once). `route`, `anchor` and the anchor candidates are computed; a `same_as` must name a candidate, and a row with candidates but no `same_as` gets them as `related`. The cell's earlier `unverified` rows become `superseded`. Defect and cleanup rows are minted `unverified`; an advisory row `open`, or `merged` with `outcome = "suppressed"` when its `same_as` names a dismissed, deferred, parked, backlogged or converted finding. Prints `{ok, run, cell, findings: [{id, kind, severity, route}], superseded, refused}`.

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

Upserts a verdict on (finding, `by`, `seat`): an identical replay writes nothing and a changed one moves to the end, where readers take the latest from. A finding with no `same_as` takes its status at once — `confirmed` or `plausible` → `open`, `refuted` → `refuted`, `unsettled` → `unverified`, re-routed to `verifier`; a panel decides once two seats agree (two non-refuted → `open`, two refuted → `refuted`). A finding with a `same_as` stays `unverified` for `resolve`. Only `unverified`, `open` and `refuted` findings take a status from a verdict. Prints `{ok, verdicts: [{finding, verdict, reroute?}], refused, unchanged}`.

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
| `--rationale` / `--rationale-file` | text \| path | Why, at most 600 characters; refused when absent, as `--citation` is. | none |
| `--strike-same-as` | bool | Clear the finding's `same_as`, so it resolves on its own verdict. | off |
| `--ndjson` | `-` \| path | A batch of verdict rows, one per line. Excludes every single-verdict flag. | none |

Refused: a verdict on an advisory or `superseded` finding; `severity` on a non-defect; a checker's `plausible`; a checker's `refuted` on a row from a cell that is not a `review-sweeper` cell; `unsettled` from anyone but the checker.

### `tomlctl review resolve`

Resolves every `unverified` finding of the run whose `same_as` has complete verdicts, passing until none resolves, so a same-run target always settles first; a `merged` target is followed through `merged_into`. By the target's status and the row's call:

| Target status | `confirmed` | `plausible` | `refuted` |
|---|---|---|---|
| `open`, `exported` | repeat | repeat | contested |
| `refuted` | reopened | contested | repeat |
| `resolved` | regression | contested | repeat |
| `dismissed`, `deferred`, `parked`, `backlogged`, `converted` | suppressed | suppressed | suppressed |

A repeat appends the run to the target's `seen_in` and refreshes its `line` and `evidence`; a contested or reopened target gains the deciding verdicts, and a reopened one returns to `open`. The row becomes `merged` with `merged_into`, except a regression, which stays `open` with `related = [target]`. Prints `{ok, resolved: [{finding, outcome, target, status}], pending: [{finding, same_as, reason}], unchanged}`.

```bash
tomlctl review resolve --slug <slug> --run U1 --wave 2
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--run` | run id | The run resolved. | required |
| `--wave` | N | Only findings from cells of this wave. | every wave |

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

The orchestrator's view of one run, as a single object: `run` (and `since_wave`); `counts` by `kind`, `lens` and `status`; `cells` (`status` counts, `unsealed` ids, the `notes` of `thin` and `uncovered` cells); `unverified` (`[{id, kind, route, cell}]`); the coverage accounting `unselected`, `unsliced` and `disabled`, the first two needing a waiver before `run finish`; `contested` rows with their `target`; `refuted_criticals`; `audit_sample` (`confirmed_defects`, `refuted_defects` and `checked_cleanup` ids, two each, the cleanup including a sweeper row when there is one); and the open `advisory` rows. Rows are projected `--for orchestrator`, so the digest never carries `evidence`.

```bash
tomlctl review digest --slug <slug> --run U1 --since-wave 2
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--run` | run id | The run summarised; an unknown run is `kind=not_found`. | required |
| `--since-wave` | N | Only cells and findings from this wave onward. | every wave |

### `tomlctl review report`

Writes the run's markdown report — defects by severity, the Needs-a-plan group, cleanup by file, advisory with dispositions, the coverage map — from the store alone, so one store always writes the same bytes. The report is derived and has no sidecar; `--out` must lie under the repo's `.claude/`. Prints `{path, counts}`, where `counts` is the report's counts line (`defect n · cleanup n · advisory n · refuted n · suppressed n · unverified n · cells n · uncovered n`).

```bash
tomlctl review report --slug <slug> --run U1 --out <run-dir>/report.md
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--run` | run id | The run reported. | required |
| `--out` | path | Where the report is written. | required |

### `tomlctl review stats`

Rates per lens, probe or run over one store or all of them. Writes nothing. Prints `{ok, stores: [paths], by?, rows?, outcomes?, escapes?, calibration?}`.

- **Rows** (`--by`, or no section flag): `key`, `families`, `runs`, `cells`, `findings` by kind, `yield_per_cell`, `verified`, `refuted`, `refute_rate`, `audited`, `overturned`, `audit_overturn_rate`, `dismissed`, `dismissal_rate`, `thin`, `uncovered`, `uncovered_rate`; per lens also `hashes`, `waived` and `waiver_rate`, per probe tag `probes`. A rate over zero is `null`. A probe groups under the tag its `reason` opens with (`nearest <tag>:`), else `untagged`.
- **`outcomes`**: per lens and kind, `exported`, `linked`, the v1 statuses, the mapped v2 statuses and `wontfix_reasons`, through `v2_ref` with `sync-v1`'s mapping.
- **`escapes`**: `count`, `by_concern` and `findings` minted in a file and concern an earlier run's cell had sealed `covered`.
- **`calibration`**: per area, the top exemplar sentences with `count` and `sources`, drawn only from user dismissals, v1 wontfix reasons and audit overturns.

```bash
tomlctl review stats --all --by lens
tomlctl review stats --slug <slug> --outcomes --escapes --calibration --exemplars 3
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` / `--all` | name \| bool | One store, or every flow and flow-less store. | required, one of |
| `--family` | `code` \| `perf` \| `plan` | Only this family. | every family |
| `--by` | `lens` \| `probe` \| `run` | The grouping axis of `rows`. | `lens` |
| `--outcomes` | bool | Add the v1 apply outcomes. | off |
| `--escapes` | bool | Add the coverage escapes. | off |
| `--calibration` | bool | Add the calibration exemplars. | off |
| `--exemplars` | N | Exemplars per area; needs `--calibration`. | 5 |

## Dispositions and export

### `tomlctl review triage`

Applies one disposition to each id. A row already in the requested end state is `unchanged`; under `--expect-status`, an id no longer at that status lands under `skipped_stale` as `{id, field, expected, found}`, checked inside the locked write. A move clears every earlier disposition's companions. A `superseded`, `merged`, `converted` or `exported` finding is never moved. Prints `{ok, mode, applied: [{id, from, status}], unchanged, skipped_stale, refused, minted?}`.

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
| `--park` / `--park-file` | text \| path | Status → `parked`; advisory only. | one disposition required |
| `--resolve` / `--resolve-file` | text \| path | Status → `resolved`, recording `resolution` and today's `resolved`. | one disposition required |
| `--backlog-ref` | `B-<hex>` | Status → `backlogged`, recording the backlog item id. | one disposition required |
| `--convert` | `cleanup` \| `defect` | Mint the advisory's replacement from `--ndjson`; status → `converted`. Exactly one advisory id. | one disposition required |
| `--ndjson` | `-` \| path | The replacement as one JSON object; with `--convert` only. | none |
| `--expect-status` | status | Move only the ids still at this status. | no check |

`--convert` defaults the replacement's lens, anchor and summary from the advisory, validates and routes it, records an `orchestrator` verdict on it and leaves it `unverified` for its route; `minted` reports `{id, kind, severity, route, status}`, and a replay reports the replacement already minted.

### `tomlctl review export-v1`

Exports open defect and cleanup findings into a v1 ledger for `/review-apply` or `/optimise-apply`. The ledger's family comes from `--to`: `review-ledger.toml` or a path under `.claude/reviews/` takes `code` findings with `R` ids; `optimise-findings.toml` or a path under `.claude/optimise-findings/` takes `perf` findings with `O` ids.

- **Refused**, with a reason: the plan family, advisory, any status but `open`, another family than the ledger's, `fix_shape = needs-plan`, `effort = large`.
- **`--all`** takes every open defect and cleanup finding of the ledger's family in this store — there is no run filter — and holds back, under `held`, test removals and cleanup whose latest verdict is `plausible`; those export only by explicit id.
- **Rows.** A defect exports with its latest severity (`major` → `warning`, `minor` → `suggestion`), cleanup as `suggestion`; `category` is the bound lens's `v1_category`, or `quality` / `algorithm` for a probe or check cell; `description` is built from the implementer projection. The v1 row carries `v2_ref = "<store path>#F<n>"`.
- **Matching.** A v1 row already carrying the `v2_ref` is `linked`. Otherwise the v1 merge rule applies (same `file`, and the same `symbol` or exact `summary`): an open match is `reused` with its `rounds` bumped, a fixed or applied match gets a new id with `related` (`regression`), and a deferred, wontfix, wontapply or verified-clean match is skipped. Anything else is `minted`.
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

## Catalogue and schema

### `tomlctl review catalogue`

Merges the built-in and project layers and applies the project config. Takes no store. Prints `{tree_sha, config_sha, entries}`, one entry per lens, facet, method and check: `type`, `id`, `hash`, `status`, `families`, `mode`, `produces`, `activation`, `apply_when` (`signals`, `paths`, `min_depth`, `always_at`), `matched`, `attaches_to`, `path`, `layer`, `disabled`.

```bash
tomlctl review catalogue --check
tomlctl review catalogue --family code --signals lang:rust,public-api-change
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--builtin` | dir | The built-in catalogue root. | `<claude dir>/skills/review-v2-lenses` |
| `--project` | dir | The project layer. | `.claude/review-lenses` under the repo root |
| `--family` | `code` \| `perf` \| `plan` | Only entries of this family. | every family |
| `--signals` | `a,b` | Mark the entries these signals match, under `matched`. | none |
| `--vocabulary` | bool | Print `{signals: [{name, kind, meaning, detect, hint, layer}]}` instead. | off |
| `--owners` | bool | Print `{owners: [{family, tag, owners}]}`, the concern-ownership map, instead. | off |
| `--check` | bool | Validate; print `{"ok":true}`, or exit 1 listing every error. | off |

`--check` covers unique ids, the frontmatter schema per type, signals and tags drawn from the merged vocabulary, one owner per (family, tag) matching each lens's Owns section, existing `methods` and `attaches_to` targets, a Probes section per declared family, config and scope ids that exist, the `project:` prefix rule for project additions, every `contains` pattern compiling, and `v1_category` valid per family. `run start` runs the same validation.

### `tomlctl review schema`

Prints a JSON Schema (draft 2020-12) built from the field catalogue. `finding` is the row `review add` accepts — closed, with each kind's required set; `cell-return` is what a cell agent returns, `{cell, findings: [{id, kind, severity, route}], status, escalate?}` — `add`'s cell and findings with the status `cover` sealed; `verdict-return` is `{finding, verdict, reroute?}`.

```bash
tomlctl review schema finding --kind cleanup
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | `finding` \| `cell-return` \| `verdict-return` | The schema printed. | required |
| `--kind` | `defect` \| `cleanup` \| `advisory` | Narrow `finding` to one kind; refused on the other targets. | every kind |
