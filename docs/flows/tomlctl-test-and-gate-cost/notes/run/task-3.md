Status: done — gate-stamp.sh has a step table with per-step stamps, and the hook's gate takes a step id

```toml run-note
files_touched = ["scripts/gate-stamp.sh", ".githooks/pre-commit", "CLAUDE.md"]

[[deviations]]
what = "`run` keeps going when a step's roots changed while it ran: it writes no stamp for that step, runs the remaining steps, and exits 1 at the end naming every step left unstamped"
why = "The design says only that such a step gets no stamp. Stopping is reserved for a failing command. Running on means the other steps can still earn their stamps, and exit 1 still tells the caller to rerun."
means = "design/gates.md#stamps"

[[deviations]]
what = "`check <id>` and each per-step digest list only the roots of the steps asked for, not all six roots"
why = "The detail asks for one listing and one hash pass shared by all steps. `digests` does that for every id it is given, so `check` with no id and `run`'s pre-flight scan once. The hook asks for one step at a time, so its listing covers only that step's roots, which is less work than scanning all of them."
means = "detail: compute one file listing and one hash pass for all steps"

[[checks]]
command = "grep -q 'plan-corpus' scripts/gate-stamp.sh && grep -q 'dispatch-tests' scripts/gate-stamp.sh"
result = "pass"

[[checks]]
command = "grep -q 'gate fmt-tomlctl' .githooks/pre-commit && ! grep -q 'gate cargo' .githooks/pre-commit"
result = "pass"

[[checks]]
command = "bash scripts/gate-stamp.sh check no-such-step; test $? -eq 2"
result = "pass"

[[checks]]
command = "grep -q '.claude/backlog.toml' scripts/gate-stamp.sh"
result = "pass"

[[checks]]
command = "bash scripts/gate-stamp.sh command fmt-tomlctl | grep -q 'cargo fmt'"
result = "pass"

[[checks]]
command = "bash -n .githooks/pre-commit"
result = "pass"

[[checks]]
command = "copy of gate-stamp.sh in the scratchpad with each command prefixed by `true`: run, check per step, untracked docs/plans probe file, then one step's command set to `false`"
result = "pass"
```

## What changed

- `scripts/gate-stamp.sh`: one step table: `STEPS` gives the run order, and the associative arrays `COMMAND` and `ROOTS` are keyed by id. The ids are `fmt-tomlctl`, `dispatch-tests`, `plan-corpus` (which also reads `.claude/backlog.toml`), `fmt-glimpse` and `glimpse-clippy`. `digests <id>...` lists the union of the given steps' roots once and hashes them in one `git hash-object --no-filters --stdin-paths` pass. It then folds each step's sha256 from `step <id>`, `command <cmd>` and only the entries under that step's roots. The forms are `run [<id>...]`, `check [<id>]` and `command <id>`. `run` removes the legacy `gate-stamp` file and stops at the first failing command; the failing step's stamp is removed before it runs. A step that passes is stamped only when its digest is the same before and after it ran. Stamps are one file per step under `gate-stamp.d/` in the git directory. An unknown id, or a malformed call, exits 2 with usage.
- `.githooks/pre-commit`: `gate <id>` reads the command words from `gate-stamp.sh command <id>` with `read -ra` and runs `check <id>`. On a match it prints `pre-commit: <id> skipped, its gate stamp matches this working tree`; otherwise it runs the command and blocks on failure, naming the command. If the step lookup fails, the commit is blocked. `STAMP_STATE` is gone. The hook writes no stamp. The staged-path triggers, the verifiers, the doc gate and the sidecar comparison are unchanged.
- `CLAUDE.md`: under Developer setup, one sentence is added to the hook paragraph saying the cargo steps are called by id and take their commands from the script. The gate-stamp paragraph is rewritten to describe the step table, the per-step stamps, `run <id>`, `check <id>`, `command <id>` and the hook's per-step skip message.

The two gate-owned accept checks were not run, because they need a real passing `run`. I exercised the same logic with a stubbed copy of the script whose commands were prefixed by `true`. After the run, all five stamps matched. An untracked `docs/plans/.gate-stamp-probe.md` voided only `plan-corpus`, and the format and clippy stamps still matched. The real script's `check fmt-tomlctl` failed against the stamp written by the copy with the different command, which shows that a command change voids the stamp. A step changed to `false` exited 1 and left no stamp. I removed the stub stamps afterwards. The stubbed `run` also removed any legacy `gate-stamp` file in the git directory, so the first commit after this lands pays the cargo steps once, or `bash scripts/gate-stamp.sh run` restamps.

The scripts are CRLF on disk. I checked that the command `command` prints contains no carriage return, so the hook's `read -ra` gets clean words.
