# tomlctl — plan probes reference

`tomlctl plan probes` over a v2 flow's task store: its flags, the scripts it emits, what `--record`
accepts, and the verdicts it stores on `accept` rows and on premises. The other `plan` verbs, the
shared write bundle it carries and the check classes that read its verdicts are
[plan.md](plan.md); where a `probe` sits in `tasks.toml` is [tasks-store.md](tasks-store.md).

## Contents

- [`plan probes`](#plan-probes)
- [Accept scripts](#accept-scripts)
- [Premise scripts](#premise-scripts)
- [Recording accept verdicts](#recording-accept-verdicts)
- [Recording premise verdicts](#recording-premise-verdicts)
- [Gate probes](#gate-probes)

## `plan probes`

`--emit` prints a Bash script that runs controls; `--record` turns that script's captured stdout into
stored verdicts. Without `--premises` or `--gates` both work on the `accept` rows, with `--premises`
on the spec's `premises`, and with `--gates` on the milestones' [gate probes](#gate-probes). Exactly
one of `--emit` and `--record` is required; neither or both, a bare `--premises` included, is a clap
usage error, exit `2` outside the `--error-format json` envelope. The row modes cover only rows with
status `pending`.

```bash
tomlctl plan probes --slug <slug> --emit > <script>
tomlctl plan probes --slug <slug> --record <capture>
tomlctl plan probes --slug <slug> --emit --premises --task <N1,N2> > <script>
tomlctl plan probes --slug <slug> --record <capture> --premises --task <N1,N2>
tomlctl plan probes --slug <slug> --emit --gates --milestone <id> > <script>
tomlctl plan probes --slug <slug> --record <capture> --gates --milestone <id>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--emit` | — | Print the probe script. | — |
| `--record` | path | Store the verdicts from the script's captured stdout. | — |
| `--premises` | — | Work on the premises instead of the `accept` rows, with either `--emit` or `--record`. | off |
| `--gates` | — | Work on each milestone's stored gate probes instead of task rows. Refused with `--premises` or `--task`. | off |
| `--milestone` | id | Restrict either mode to this checkpoint's pending rows. An id in neither `[[checkpoints]]` nor `[[milestones]]` is refused with `kind=validation`. | all |
| `--task` | ids, comma-separated | Restrict either mode to these tasks' pending rows. An id no row carries is refused with `kind=validation`. With `--milestone`, a row must satisfy both. | all |

Record with the same `--premises`, `--gates`, `--milestone` and `--task` the script was emitted with: the scope
decides which controls the capture must cover.

tomlctl never runs a stored command: `Bash(tomlctl *)` is pre-approved, so a verb executing stored
strings would widen that approval to arbitrary commands. The caller runs the script, and the commands
in it are repo code run with the caller's rights.

Every script opens with `head<TAB><sha>` and changes to the repo root, so stored commands are
repo-root-relative. Each command reaches the script as one single-quoted word, and a NUL in one
refuses the emit. A task that is not pending, is outside the requested milestone, or is not among the
`--task` ids gets `# skipped task <id>: status is not pending`, `# skipped task <id>: not in the requested milestone`
or `# skipped task <id>: not a requested task`, and none of its strings reach the script. Each
control prints one row of `<task>:<index><TAB><control><TAB>sha256=<hex><TAB>rc=<n><TAB><stdout><TAB><stderr>`,
with every column folded onto one line and `<hex>` the SHA-256 of the stored command.

## Accept scripts

Only a row whose [effective owner](tasks-store.md#accept-owner) is `worker`, explicitly marked
`writes_build = false` and with a non-empty `check`, runs. Any other row gets a `# skipped` comment
(`empty check`, `writes_build is not false` or `run_by is not worker`) and none of its strings reach
the script: a check the gate runs, or a `covered` one, is never probed. A row's `check` runs as the negative
control (`neg`) and its `positive`, when set, as the positive control (`pos`). `<index>` counts the
spec's `accept` rows.

## Premise scripts

One line per premise whose `at` is `base`, or `after:<id>` with task `<id>` `done`, on the same
pending, in-scope tasks. Its control is `premise` and `<index>` counts the spec's `premises`. A premise
gets no line, only a comment, when:

| Comment | Premise |
|---|---|
| `# skipped <task>:<index>: empty check` | Its `check` is blank. |
| `# skipped <task>:<index>: task <id> is not done` | `after:<id>`, and task `<id>` is any other status. |
| `# skipped <task>:<index>: no task <id> in the store` | `after:<id>`, and no row carries `<id>`. |
| ``# skipped <task>:<index>: `at` is neither `base` nor `after:<number>` `` | Any other `at`, including a non-canonical id such as `after:007`. |

`rc=0` means the premise holds.

## Recording accept verdicts

`--record` needs a capture covering exactly the probeable controls of the pending rows in scope, each
carrying the digest of the command the store holds now. A truncated run, an edited `check` or
`positive`, a capture with no digest, a capture naming a row outside that set, or one holding a
`premise` row is refused with `kind=validation` and writes nothing; re-run `plan probes --emit`. Other
rows' stored probes stay as they were. The digest catches a stale capture, not a forged one. It writes
each row's `probe` as `at` (the capture's `head`), `neg_rc`, `pos_rc` and `verdict`, and prints:

```json
{"ok":true,"recorded":[{"task":1,"accept":0,"verdict":"healthy","reason":""}]}
```

Each row's `reason` is `""` when healthy and otherwise names the control and the rule it hit. It is
printed only and never stored.

| Verdict | Read from |
|---|---|
| `healthy` | A forward whose `check` fails now and whose positive control, if any, passes. A falsifier whose `check` passes. A guard whose `check` passes beside a discriminating row. |
| `vacuous` | A forward whose `check` already passes: it tests nothing, or its premise is false. |
| `unsatisfiable` | A forward whose `check` fails and whose positive control fails too. |
| `permanently-green` | A falsifier whose `check` fails. |
| `guard-alone` | A guard that passes with no discriminating row beside it. A discriminating row is a forward or falsifier with a `check` that is unprobed or `healthy`. |
| `broken` | Either control's stderr names `command not found`, `fatal`, `syntax error`, `unexpected EOF`, or `No such file` for a path no row declares. Also a guard that fails, or an unknown `kind`. |

## Recording premise verdicts

`--record --premises` needs a capture covering exactly the premises the matching
`--emit --premises` checks, each carrying the digest of the `check` the store holds now. A capture with
fewer rows than that (a script cut short by a timeout), a missing or extra premise, an edited `check`,
or a capture holding a `neg` or `pos` row is refused with `kind=validation` and writes nothing. It
writes each premise's `probe` as `at` (the capture's `head`), `neg_rc` (the check's exit code) and
`verdict`, and prints:

```json
{"ok":true,"recorded":[{"task":1,"premise":0,"verdict":"holds"}]}
```

| Verdict | Read from |
|---|---|
| `holds` | The check exited 0. |
| `fails` | The check exited non-zero. |
| `broken` | The check's stderr names `command not found`, `fatal`, `syntax error` or `unexpected EOF`, whatever it exited with. |

`plan view --for worker` shows a recorded premise verdict beside the premise, and
`plan drift` lists a task's premises under `affected` for re-checking.

## Gate probes

A gate probe is a cheap command that fails where a milestone gate cannot pass in this environment,
such as the gate's own `--dry-run`; the gate itself is never its probe.
`plan milestone add|update --gate-probe '<gate> :: <probe>'` stores it in the milestone's
`gate_probes` (see [plan.md](plan.md#plan-milestone-add)), and `plan check` reports
`plan/gate-probe-unknown` once its gate is gone.

`--emit --gates` covers every milestone, or the one `--milestone` names, whatever its tasks' status.
Each stored probe gets one line keyed `<milestone>:<index>`, `<index>` counting the milestone's
`gate_probes`, with control `gate`; a blank probe gets `# skipped <milestone>:<index>: empty probe`.
The milestone id reaches the script bare, so a milestone holding probes whose id is not ASCII
letters, digits, `.`, `_` or `-` is refused.

`--record --gates` needs a capture covering exactly the probes that script runs, each with the digest
of the probe the store holds now; a short or extra capture, an edited probe or any row whose control
is not `gate` is refused and writes nothing. It stores each probe's `verdict` by the
[premise rules](#recording-premise-verdicts) (`holds`, `fails`, `broken`) and `at` (the capture's
`head`), and prints:

```json
{"ok":true,"recorded":[{"milestone":"A","gate_probe":0,"verdict":"holds"}]}
```
