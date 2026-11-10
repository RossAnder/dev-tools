# Survey — stage 2

The survey profiles the change and the project so stage 3 can compose coverage from evidence rather than from a template. It runs in four steps, in this order:

1. `review survey` evaluates the vocabulary's deterministic detectors and records the true signals with their roots.
2. One `review-surveyor` (Haiku) reads those signals back from the store, proposes judged signals, runs the surveyor checks, resolves the baseline commands, and writes `profile.md` and `pack.md`.
3. You read its digest; stage 3 can start from here.
4. One `verification` dispatch carries every resolved baseline command with `keep_going: true`, and each command's result is recorded in the run's `baselines`. Stage 4 does not start until every key has an entry.

Pass `--scope <scope>` in place of `--slug <slug>` for a flow-less store in every command below. `<run>` and `<run-dir>` are the run id and the `dir` that `review run start` printed.

A plan-family run on a v2 flow, one whose `.claude/flows/<slug>/plan.toml` exists, is in **flow mode**. Its scope is `docs/flows/<slug>/brief.md` plus every `docs/flows/<slug>/design/*.md`, not a plan file, and its records live in the flow's `tasks.toml` and `plan.toml`. No task-store import runs, because the store is already canonical and `tasks import-plan` refuses a v2 flow. The sections below mark what changes in flow mode; the rest holds as written.

## 1. Detectors

```bash
tomlctl review survey --run <run> --slug <slug>
```

The verb evaluates every `stack`, `diff` and `health` signal in the merged vocabulary, built-in plus any project `project:` signals, with no agent involved:

- **stack** signals over `git ls-files`: manifest and project-file globs, content regexes, manifest key paths (TOML and JSON only; MSBuild XML through globs plus regexes). Each records the directories it matched as `roots`, so a polyglot repo gets separate roots per stack and a facet attaches only where its stack lives.
- **diff** signals over the run's diff inside the scope, `base..head` when the run records a `head` and `base..HEAD` otherwise: renames and removals, public API, dependencies, schema, query layer, async, process-heavy code, build scripts and hooks, package lifecycle scripts, frontend, docs and tests touched.
- **health** signals over the tree and `git log` windows: test growth, test ratio, doc growth, churn hotspots, finding hotspots, backlog clusters. Churn-hotspot evidence names only files in the run's scope.

The detector semantics — glob and regex rules, `count` literals, windows, thresholds and how roots are derived — are defined in the `review-v2-lenses` skill's Detectors section. You do not load that skill during a run; it is for catalogue authors.

**The store holds the signals.** `tomlctl review run show <run> --slug <slug>` reads them back, each signal's `roots` limited to the roots that meet the run's scope, with the rest counted in `roots_outside_scope`. The surveyor reads them that way by run id, so nothing from this output is copied into its prompt. A detector that could not be evaluated (git unavailable, a manifest that does not parse) is printed as `unknown` with its reason. `unknown` is not false: when an `unknown` signal would have activated a lens, decide at stage 3 whether to force that lens with `--lens +id` or to waive it, naming the unknown signal in the reason.

## 2. The surveyor dispatch

Dispatch one `review-surveyor` with the preamble and surveyor prompt from `agent-waves.md` (The prompts; Surveyor prompt): the `ledger:` line for a flow-bound run, the run, `wave: 0` (no cell wave has run yet), the store flag, the pack and run-directory paths, then the family, the scope, and for the plan family either the `plan:` path of a markdown plan or, in flow mode, a `flow: <slug>` line in its place. The prompt carries no signals: the surveyor reads them with `tomlctl review run show <run> <store flag>`. Dispatch it one-shot in the background and end your turn; it counts toward the 16 running agents.

The surveyor works only inside the run directory and the store:

- it proposes **judged** signals (`trust-boundary`, `hot-path`, `new-seam`, any `project:` judged signal) as candidates with 1 to 5 evidence paths each, written to the run's `signals` with `accepted` unset. It never accepts or rejects one;
- it runs every check whose executor is `surveyor` and writes each result as a pack fence;
- it resolves, without running, the command of every check whose executor is `verification`: a built-in check by walking its `discover` ladder, a project check by taking the `executor` and `command` that `tomlctl review catalogue` prints for it;
- it writes `profile.md` (at most 150 lines) and `pack.md`, then returns a digest of at most 40 lines.

When the digest is `missing: <what>`, the prompt lacked the run, the store flag or the run directory: fix the prompt and dispatch once more with the identical preamble.

## 3. Reading the digest

The digest has six parts. Each feeds a later decision:

| Part | What you do with it |
|---|---|
| `candidates:` | accept or reject each at stage 3 (`compose.md`, Judged candidates) |
| `hotspots:` | the most-changed files in scope; stage 3 gives each its own slice when the proposal has not |
| `verification:` | the commands of the one baseline dispatch (section 6); a `none — <reason>` line is recorded as is |
| `checks:` | a surveyor check `not run` leaves its fence saying why; note it for the close summary |
| `unmeasured:` | a gap in the profile; consider a probe or a waiver that names it at stage 3 |
| `refused:` | a store write the surveyor could not land; a refused judged signal is simply not a candidate |

Read `profile.md` too: files by role and subsystem, signals with roots and measurements, the candidates' evidence, hotspots, and for the plan family the task list with effort tags, from `tomlctl tasks list --slug <slug>`. In flow mode the profile also lists the brief and design files and the flow's decisions and contracts, read from `tomlctl plan view --slug <slug> --for human`. Do not read `pack.md`; it is the agents' preamble, not your input.

## 4. Checks by executor

Every catalogue check has one executor, which decides who runs it and where its result lands:

| Executor | Run by | When | Result |
|---|---|---|---|
| `tomlctl` | the verb that owns it | inside the verb's own run | the verb's output |
| `surveyor` | review-surveyor | stage 2, during its dispatch | a fence in `pack.md` |
| `verification` | the `verification` agent, one dispatch carrying every baseline command | stage 2, after the surveyor | a log path in the run's `baselines`; the pack fence points to it |
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

In flow mode the plan checks change as follows:

- `store-check` runs `tomlctl plan check --slug <slug>` and `tomlctl tasks check --slug <slug>`, in place of the import dry run and `tasks check --plan`.
- `format-contract` is not run, and its fence says so.
- `break-set-baseline` resolves `none — <reason>`, since task rows carry no predicted break sets, so no baseline is dispatched for it.
- `two-control-probe` reads the recorded probe verdicts rather than running its helper, and `falsifiability` and `acceptance-contradiction` read the specs and contracts.

Each check file's own v2 flow section is authoritative.

A project's `[[checks]]` in `.claude/review-lenses/config.toml` are always `verification` checks with their command given, run when the run's family is in their `families` and their `paths` meet the scope; in this repo that is `harness-gates`. The surveyor takes each one's `executor` and `command` from `tomlctl review catalogue` and lists it under `verification:` with the built-in ones. When the digest omits a project check that the catalogue lists for the run's family and scope, add its catalogue `command` to the baseline dispatch all the same. The authoritative set for a run is the catalogue the run validated, so treat these tables as the expected shape: when the digest lists a check this table does not, run it by its executor all the same.

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

## 6. Baselines — the verification dispatch

The `verification` agent runs an ordered command list in sequence, one command after another, and writes logs only to its own scratch directory. It stops at the first failing command unless the prompt sets `keep_going: true`; a failing lint must not hide the next baseline, so **one dispatch carries every baseline command with `keep_going: true`**. Running them in sequence inside that one agent also keeps them off each other's build lock: the commands share the build's target directory, and parallel builds serialise on its lock and thrash its cache.

What each carrier expects to run:

| Carrier | Baselines |
|---|---|
| `/review-v2` | `lint-baseline`, plus every project check for `code` whose `paths` meet the scope |
| `/optimise-v2` | `profile-baseline`, plus every project check for `perf` |
| `/review-plan-v2` | `break-set-baseline` (a `none — <reason>` line in flow mode), plus every project check for `plan` |

The digest's `verification:` lines are the actual list: every line that resolved to a command becomes one item of the dispatch's `commands:`, in the order listed. A check can resolve to more than one command — an ecosystem rung yields one per stack root that meets the scope, and `break-set-baseline` one per predicted break set — so each line carries a key: the check id for its first command, then `<id>-2`, `<id>-3` and so on, in ladder or plan order.

**The prompt.** The `ledger:` line first for a flow-bound run, then the agent's own input shape:

```text
ledger: .claude/flows/<slug>/review-v2.toml
commands:
- <the first resolved command>
  timeout: 540
- <the next resolved command>
  timeout: 540
keep_going: true
transient:
- rust-lld: failed to write output.*[Pp]ermission denied
```

Give every command its own `timeout` line and keep each at or below 540, the agent's default: a larger budget holds only when the agent finds a `timeout` binary to run the command detached, and without one its Bash call's 600-second ceiling cuts the command off whatever the budget says. Split a slow baseline into one command per crate or test binary rather than raising the budget, and give each piece its own key, numbered on from the check's last key as above. Keep your own list of each command's key in `commands:` order; the agent reports its commands in that order. Pass `transient:` patterns only when the project names them, in a plan's Verification Commands or its CLAUDE.md. Dispatch in the background and end your turn; the agent counts as one toward the 16 running. Stage 3 work may proceed while the baselines run.

**Recording.** When the return arrives, record every key from it in one call, each command's log path under its key:

```bash
tomlctl review run update <run> --slug <slug> --set-json 'baselines={"lint-baseline":"<log-path>","lint-baseline-2":"<log-path>","harness-gates":"<log-path>"}'
```

- Record each returned log path whatever its outcome. A red lint or a failing break-set test is information for the finders, not a halt; a `timeout` log is partial, so record its path and note `timeout` in the close summary.
- A line the surveyor resolved to `none — <reason>`, or a command the return gave no log for, is recorded as that text in the same call: `"profile-baseline":"none — no benchmark harness found"`.
- `baselines` merges per key, so a repeated call replaces only the keys it names. A retry after `os error 5` is harmless.
- Read the recorded set back with `tomlctl review run show <run> --slug <slug> --header --get baselines`.

## 7. The stage 3 → 4 barrier

Stage 4 starts only when every key the digest listed under `verification:` has a recorded `baselines` entry, a path or a `none — <reason>`, and so does every key a split produced (section 6): a split baseline is ready only when each of its pieces is recorded. Check the set with `tomlctl review run show <run> --slug <slug> --header --get baselines` against the digest's keys. Composition does not need the logs, so finish stage 3 while the baselines run, and launch the wave once every entry is recorded. A finder launched before its baseline is recorded reads no lint fence and re-reports what the baseline already says.
