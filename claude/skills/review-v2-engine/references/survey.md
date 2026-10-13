# Survey — stage 2

The survey profiles the change and the project so stage 3 can compose coverage from evidence rather than from a template. It runs in four steps, in this order:

1. `review survey` evaluates the vocabulary's deterministic detectors and records the true signals with their roots.
2. One `review-surveyor` (Haiku) proposes judged signals, runs the surveyor checks, resolves the baseline commands, and writes `profile.md` and `pack.md`.
3. You read its digest; stage 3 can start from here.
4. One `verification` dispatch per resolved baseline command, one after another, each recorded in the run's `baselines`. Stage 4 does not start until every one has an entry.

Pass `--scope <scope>` in place of `--slug <slug>` for a flow-less store in every command below. `<run>` and `<run-dir>` are the run id and the `dir` that `review run start` printed.

## 1. Detectors

```bash
tomlctl review survey --run <run> --slug <slug>
```

The verb evaluates every `stack`, `diff` and `health` signal in the merged vocabulary, built-in plus any project `project:` signals, with no agent involved:

- **stack** signals over `git ls-files`: manifest and project-file globs, content regexes, manifest key paths (TOML and JSON only; MSBuild XML through globs plus regexes). Each records the directories it matched as `roots`, so a polyglot repo gets separate roots per stack and a facet attaches only where its stack lives.
- **diff** signals over the run's `base..HEAD` diff inside the scope: renames and removals, public API, dependencies, schema, query layer, async, process-heavy code, frontend, docs and tests touched.
- **health** signals over the tree and `git log` windows: test growth, test ratio, doc growth, churn hotspots, finding hotspots, backlog clusters.

The detector semantics — glob and regex rules, `count` literals, windows, thresholds and how roots are derived — are defined in the `review-v2-lenses` skill's Detectors section. You do not load that skill during a run; it is for catalogue authors.

**Its output is your only copy of the signals.** No read-only verb returns a run's recorded signals afterwards, and the surveyor needs them, so keep each printed signal's `name` and `roots` exactly as printed for the surveyor prompt. A detector that could not be evaluated (git unavailable, a manifest that does not parse) is printed as `unknown` with its reason. `unknown` is not false: when an `unknown` signal would have activated a lens, decide at stage 3 whether to force that lens with `--lens +id` or to waive it, naming the unknown signal in the reason.

## 2. The surveyor dispatch

Dispatch one `review-surveyor` with the preamble and surveyor prompt from `agent-waves.md` (The prompts; Surveyor prompt): the `ledger:` line for a flow-bound run, the run, `wave: 0` (no cell wave has run yet), the store flag, the pack and run-directory paths, then the family, the scope, the plan path for the plan family, and the `signals:` block copied from step 1's output. Dispatch it one-shot in the background and end your turn; it counts toward the 16 running agents.

The surveyor works only inside the run directory and the store:

- it proposes **judged** signals (`trust-boundary`, `hot-path`, `new-seam`, any `project:` judged signal) as candidates with 1 to 5 evidence paths each, written to the run's `signals` with `accepted` unset. It never accepts or rejects one;
- it runs every check whose executor is `surveyor` and writes each result as a pack fence;
- it resolves, without running, the command of every check whose executor is `verification`, by walking the check's `discover` ladder;
- it writes `profile.md` (at most 150 lines) and `pack.md`, then returns a digest of at most 40 lines.

When the digest is `missing: <what>`, the prompt lacked the run directory or the signals: fix the prompt and dispatch once more with the identical preamble.

## 3. Reading the digest

The digest has six parts. Each feeds a later decision:

| Part | What you do with it |
|---|---|
| `candidates:` | accept or reject each at stage 3 (`compose.md`, Judged candidates) |
| `hotspots:` | the most-changed files in scope; stage 3 gives each its own slice when the proposal has not |
| `verification:` | one baseline dispatch per resolved command (section 6); a `none — <reason>` line is recorded as is |
| `checks:` | a surveyor check `not run` leaves its fence saying why; note it for the close summary |
| `unmeasured:` | a gap in the profile; consider a probe or a waiver that names it at stage 3 |
| `refused:` | a store write the surveyor could not land; a refused judged signal is simply not a candidate |

Read `profile.md` too: files by role and subsystem, signals with roots and measurements, the candidates' evidence, hotspots, and for the plan family the task list with effort tags. Do not read `pack.md`; it is the agents' preamble, not your input.

## 4. Checks by executor

Every catalogue check has one executor, which decides who runs it and where its result lands:

| Executor | Run by | When | Result |
|---|---|---|---|
| `tomlctl` | the verb that owns it | inside the verb's own run | the verb's output |
| `surveyor` | review-surveyor | stage 2, during its dispatch | a fence in `pack.md` |
| `verification` | the `verification` agent, one dispatch per command | stage 2, after the surveyor | a log path in the run's `baselines`; the pack fence points to it |
| `cell` | a `review-finder` cell composed at stage 3 | stage 4 | findings, verified like any other |

The built-in checks:

| Check | Executor | Family | Fence or output |
|---|---|---|---|
| `store-check` | surveyor | plan | `STORE CHECK` — `tasks import-plan --dry-run` and `tasks check --plan` |
| `format-contract` | surveyor | plan | `FORMAT CONTRACT` |
| `dev-server-probe` | surveyor | code, frontend scope | `DEV SERVER` — detects a dev server; starts none |
| `lint-baseline` | verification | code | `LINT BASELINE` |
| `profile-baseline` | verification | perf | `PROFILE BASELINE` |
| `break-set-baseline` | verification | plan | `BREAK-SET BASELINE` — commands from the plan's predicted break sets |
| `two-control-probe`, `falsifiability`, `acceptance-contradiction` | cell | plan | findings from the `plan-checks` cell |

A project's `[[checks]]` in `.claude/review-lenses/config.toml` are always `verification` checks with their command given, run when the run's family is in their `families` and their `paths` meet the scope; in this repo that is `harness-gates`. The surveyor lists them under `verification:` with the built-in ones. The authoritative set for a run is the catalogue the run validated, so treat these tables as the expected shape: when the digest lists a check this table does not, run it by its executor all the same.

## 5. Pack fences

`pack.md` holds the fences every cell agent reads before its slice. Haiku agents run without CLAUDE.md, so the `CONSTRAINTS` fence is how they get the few repo rules they need. Each fence opens on a line holding only its name in capitals and closes on `END <name>`. A fence with nothing to hold says why in one line, so an absent fence is never mistaken for a clean result.

| Fence | Written from | Holds |
|---|---|---|
| `CONSTRAINTS` | repo-root and in-scope `CLAUDE.md` | at most 10 rules, quoted verbatim |
| `BACKLOG` | `tomlctl backlog list` | open backlog items in scope, at most 15 |
| `STORE CHECK`, `FORMAT CONTRACT`, `DEV SERVER` | surveyor checks | each check's result, or why it did not run |
| `CALIBRATION` | `tomlctl review stats --calibration` | exemplars by area, or `none yet` |
| `LINT BASELINE`, `PROFILE BASELINE`, `BREAK-SET BASELINE` | the run's `baselines` | only the pointer `see the run's recorded baselines`, never inline output |

The baseline fences are pointers because the logs do not exist yet when the surveyor writes the pack, and because inline output would crowd every agent's context. `review show <cell> --for finder` carries the recorded log paths, and each finder reads only the in-scope lines of those logs.

## 6. Baselines — the verification dispatches

The unchanged `verification` agent runs an ordered command list, stops at the first failing command and writes logs only to its own scratch directory. A failing lint must not hide the next baseline, so dispatch it **once per resolved command**, and **one at a time**: the commands share the build's target directory, and parallel builds serialise on its lock and thrash its cache.

What each carrier expects to dispatch:

| Carrier | Baselines |
|---|---|
| `/review-v2` | `lint-baseline`, plus every project check for `code` whose `paths` meet the scope |
| `/optimise-v2` | `profile-baseline`, plus every project check for `perf` |
| `/review-plan-v2` | `break-set-baseline`, plus every project check for `plan` |

The digest's `verification:` lines are the actual list: dispatch one per line that resolved to a command, in the order listed.

**The prompt.** The `ledger:` line first for a flow-bound run, then the agent's own input shape:

```text
ledger: .claude/flows/<slug>/review-v2.toml
commands:
- <the resolved command>
  timeout: 1500
transient:
- rust-lld: failed to write output.*[Pp]ermission denied
```

Give a slow suite a larger `timeout`. Pass `transient:` patterns only when the project names them, in a plan's Verification Commands or its CLAUDE.md. Dispatch in the background and end your turn; the agent counts toward the 16 running. Stage 3 work may proceed while baselines run.

**Recording.** Record each return as soon as it arrives, keyed by the check id, before dispatching the next:

```bash
tomlctl review run update <run> --slug <slug> --set-json 'baselines={"lint-baseline":"<log-path>"}'
```

- Record the returned log path whatever the outcome. A red lint or a failing break-set test is information for the finders, not a halt; a `timeout` log is partial, so record its path and note `timeout` in the close summary.
- A line the surveyor resolved to `none — <reason>`, or a dispatch that returned no log, is recorded as that text: `--set-json 'baselines={"profile-baseline":"none — no benchmark harness found"}'`.
- `baselines` merges per key, so each call adds one entry and a repeated call replaces its own key. A retry after `os error 5` is harmless.

## 7. The stage 3 → 4 barrier

Stage 4 starts only when every check the digest listed under `verification:` has a recorded `baselines` entry, a path or a `none — <reason>`. No read verb returns a run's baselines, so track the set yourself: each `run update` prints the keys it updated. Composition does not need the logs, so finish stage 3 while the last baseline runs, and launch the wave once its entry is recorded. A finder launched before its baseline is recorded reads no lint fence and re-reports what the baseline already says.
