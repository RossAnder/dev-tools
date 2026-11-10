# tomlctl — review runs reference

The `tomlctl review run` verbs: starting, patching, finishing, listing and reading back one run of the v2 review store. The store shape, the shared flag bundles, payload sources and output conventions are [review.md](review.md)'s, under Shared behaviour and Store shape; this file holds only the `run` verbs. How a run's bounds and scope are chosen is the `review-v2-engine` skill's call, not this one's.

## Contents

- [Bounds](#bounds) — `base`, `head` and what reads them
- [`run start`](#tomlctl-review-run-start)
- [`run update`](#tomlctl-review-run-update)
- [`run finish`](#tomlctl-review-run-finish)
- [`run list`](#tomlctl-review-run-list)
- [`run show`](#tomlctl-review-run-show)

## Bounds

A run is measured from `base` to `head`, both stored as commit shas. `head` is optional: a run without one measures to `HEAD`, read afresh by each verb. `changed_lines` and the survey's diff detectors read `base..head` (or `base..HEAD`), so uncommitted edits are invisible to them, while the project-layer check of `run start` compares `base` with the working tree; with `head` set, the survey reads each changed file's bytes at `head` rather than from the working tree. The finder assignment of a `review-finder-cf` cell carries `base`, and `head` when the run records one.

Agents read files from the working tree whatever the bounds are, so a `head` behind `HEAD` leaves later commits' edits in what an agent reads.

## Verbs

### `tomlctl review run start`

Mints `U<n>`, stamps `started`, resolves `--base` (and `--head`, when given) to a commit sha, computes `changed_lines` (lines added plus removed over `scope` in `base..head`, or the scope's tracked line count when that is 0; absent for the plan family), validates the merged catalogue — halting with every error — records the catalogue roots and shas, and creates the run directory. Prints `{ok, run, dir, base, head, changed_lines}`, `head` being `null` when `--head` was not passed.

`--head` is refused unless `base` is its ancestor (`git merge-base --is-ancestor`); a `--head` equal to `--base` is accepted.

Without `--project`, it refuses when `.claude/review-lenses` in the working tree differs from `--base` — an added, edited or deleted path, committed after `base` (including after `--head`) or uncommitted — listing those paths. The layer the run loads is the working tree's, and it can carry check commands, so a change to it would otherwise configure its own review. Passing `--project <dir>` accepts the layer, and the changed paths are recorded as `catalogue.project_layer_changed` for the orchestrator to show the user.

```bash
tomlctl review run start --slug <slug> --command review-v2 --family code --depth standard --base HEAD~3 --head HEAD~1 --set-json 'scope=@<run-scratch>/scope.json' --set substrate=workflow
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--command` | `review-v2` \| `optimise-v2` \| `review-plan-v2` | The carrier. | required |
| `--family` | `code` \| `perf` \| `plan` | The finding family. | required |
| `--depth` | `quick` \| `standard` \| `deep` | The depth envelope. | required |
| `--base` | rev | The commit the change is measured from; stored as its sha. | required |
| `--head` | rev | The commit the change is measured to, a descendant of `--base`; stored as its sha in `head`. | `HEAD`, unrecorded |
| `--builtin` | dir | The built-in catalogue root, recorded as an absolute path; the default is left unrecorded and resolved again by each later verb. | `<claude dir>/skills/review-v2-lenses` |
| `--project` | dir | The project catalogue layer; a named directory must exist (`kind=not_found`). Passing it accepts a layer that differs from `base`. | `.claude/review-lenses` when present |
| `--set` / `--set-json` / `--set-file` | `KEY=…` | Only `scope` (repo-relative paths or globs), `models` (a table), `substrate` (`workflow` \| `agent-waves`) and `rechecks` (the run id a plan re-check run re-reads; `stats --plan-outcomes` counts its findings as re-check yield). `--set-json KEY=@<path>` reads the JSON value from a file the Write tool staged. A field tomlctl computes, such as `incomplete` or `counts`, is refused as set by tomlctl. | none |

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
| `--set-json` / `--set` / `--set-file` | `KEY=…` | Only `signals`, `waivers`, `audit`, `agents` and `baselines`. `--set-json KEY=@<path>` reads the JSON value from a file. A computed field, such as `incomplete` or `waves`, is refused as set by tomlctl. | at least one |

- `signals` are upserted by `name`. They may only add `judged` entries (`source` absent or `judged`; keys `name`, `source`, `roots`, `accepted`, `evidence`) or set an existing judged entry's `accepted`; a detector entry is never changed here.
- `waivers` name exactly one of `concern` and `file`, with a `reason`; a repeated key replaces its reason.
- `agents` is a non-negative integer; `audit` and `baselines` are tables merged into the stored ones. Each `baselines` value is a log path or `none — <reason>`.

### `tomlctl review run finish`

Stamps `finished` and computes `counts` (`cells`, `uncovered`, `defect`, `cleanup`, `advisory`, `refuted`, `suppressed`). Refuses while a finding is `unverified`, a `same_as` awaits `resolve`, an unselected matched concern or unsliced scope file has no waiver, or a cell of the run is unsealed — unless `--incomplete` records why, in which case the reason and the `gaps` are printed and the reason is stored as the run's `incomplete` field, a catalogue field only `run finish` writes. A later clean finish clears an earlier `incomplete`. Prints `{ok, run, finished, counts}`.

```bash
tomlctl review run finish U1 --slug <slug> --incomplete-file <run-dir>/payloads/incomplete.txt
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | run id | The run finished. | required |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--incomplete` / `--incomplete-file` | text \| path | Finish despite open gaps, recording why. | none |

### `tomlctl review run list`

Lists the store's runs ordered by id number. Writes nothing. Prints `{ok, runs: [{id, command, family, depth, started, finished, base, head, scope, substrate, rechecks, waves, agents, incomplete}]}`; every row carries every key, `null` when the run has none, so an open run reads `finished: null` and a run without `--head` reads `head: null`. `waves` is derived from the run's cells — the highest `wave` among them, `0` when it has none — never read from a stored field; `review stats --by run` and `run show` derive it the same way. `runs` holds the rows the global output options act on.

```bash
tomlctl review run list --slug <slug> --open --where family=plan --lines
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--scope` | name | The store. | required, one of |
| `--open` | bool | Only runs without `finished`; conflicts with `--finished`. | every run |
| `--finished` | bool | Only runs with `finished`; conflicts with `--open`. | every run |

### `tomlctl review run show`

Prints one run read back from the store. Writes nothing. Prints `{ok, run, base, head, scope, depth, baselines, waves, agents, audit, signals: [{name, source, roots, roots_outside_scope, accepted, evidence}]}`: `head` and `audit` are `null` when the run has none, `baselines` is `{}`, and `waves` is derived from the run's cells as in `run list`. An unknown run fails `kind=not_found`.

Each signal keeps in `roots` only the roots that meet the run's scope — a root under a scope entry, a root above one, or the repository root (`""` or `.`) — and counts the rest in `roots_outside_scope`. A run with no scope keeps every root. `signals` holds the rows the global output options act on, so `--lines` prints the run's fields as one header line and then one line per signal, and `--header` reads the run fields alone.

```bash
tomlctl review run show U1 --slug <slug> --lines
tomlctl review run show U1 --slug <slug> --header --get baselines
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | run id | The run shown. | required |
| `--slug` / `--scope` | name | The store. | required, one of |
