Verdict: a worker could carry out tasks 1 to 5 and 8 as written; two points stop it: task 6's `opts()` instruction contradicts its own constraint (1 unactionable), and task 7 points at a production `TOMLCTL_LOCK_TIMEOUT` read that is really test code while leaving out a production `TOMLCTL_ROOT` read (1 false premise).

```toml sources
read_at = "4eeac3e0c844dd0a0bc51c08f4945cf11baadc42"
dirty = []

[[source]]
path = "tomlctl/src/output.rs"
line = 297
claim = "opts() returns &'static OutputOpts from the OPTS OnceLock"

[[source]]
path = "tomlctl/src/cli/dispatch/items.rs"
line = 174
claim = "a caller outside output.rs/lib.rs passes opts() straight into a &OutputOpts parameter"

[[source]]
path = "tomlctl/src/backlog/query.rs"
line = 74
claim = "build_query(..., output::opts()) takes out: &OutputOpts (line 181)"

[[source]]
path = "tomlctl/src/tasks/list.rs"
line = 32
claim = "build_query(count, &query_args, output::opts()) takes out: &OutputOpts (line 40)"

[[source]]
path = "tomlctl/src/cli/types/shared.rs"
line = 314
claim = "to_query_input takes out: &crate::output::OutputOpts"

[[source]]
path = "tomlctl/src/io.rs"
line = 2863
claim = "the second TOMLCTL_LOCK_TIMEOUT read sits in tomlctl_lock_timeout_clamps_at_24h, inside mod tests (starts line 2576)"

[[source]]
path = "tomlctl/src/io.rs"
line = 1936
claim = "ensure_process_root reads TOMLCTL_ROOT through std::env::var_os, a production read task 7 does not name"

[[source]]
path = "tomlctl/src/io.rs"
line = 102
claim = "resolve_lock_timeout reads TOMLCTL_LOCK_TIMEOUT uncached"

[[source]]
path = "tomlctl/tests/common/mod.rs"
line = 301
claim = "sandbox() runs git init through git_command and tolerates a git that cannot start"

[[source]]
path = "scripts/verify-shared-blocks.sh"
line = 135
claim = "the per-pair loop reports a missing file in pair order, interleaved with marker and drift errors"

[[source]]
path = "scripts/gate-stamp.sh"
line = 79
claim = "today any argument other than run/check is a usage error with exit 2; check ignores further arguments"

[[source]]
path = ".githooks/pre-commit"
line = 45
claim = "gate() checks one stamp once and runs the command given as its arguments"

[[source]]
path = "tomlctl/src/lib.rs"
line = 421
claim = "run() parses with Cli::parse and skips emit_error for a closed stdout"

[[source]]
path = "tomlctl/src/cli/dispatch.rs"
line = 157
claim = "cli::run passes std::env::args_os().skip(1) to output::check_where_placement"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/output_gate.rs"
line = 19
claim = "only output.rs, output/, cli/dispatch/tests/ and test_support.rs may write stdout or print!"

[[source]]
path = "tomlctl/tests/it.rs"
line = 6
claim = "integration tests are modules of one binary; backlog_read, flow_active, plan_drift, plan_check, tasks_render, review_anchor, review_reproduce, blocks and output_* modules exist"
```

## Unactionable

1. **Task 6, Detail: "`opts()` returns `&'static OutputOpts` today, so decide whether it returns a clone or a guard and update its callers in this file only."** Neither choice works under the task's constraint "A caller of `output::opts()` outside these two files keeps compiling without an edit there". Three callers outside output.rs and lib.rs pass `opts()` straight into a `&OutputOpts` parameter: `tomlctl/src/cli/dispatch/items.rs:174` (`to_query_input(&legacy, crate::output::opts())`, parameter `out: &crate::output::OutputOpts` at `cli/types/shared.rs:314`), `tomlctl/src/backlog/query.rs:74` (`build_query(..., output::opts())`, `out: &OutputOpts` at line 181) and `tomlctl/src/tasks/list.rs:32` (`out: &OutputOpts` at line 40). An owned `OutputOpts` does not coerce to `&OutputOpts` as an argument, and neither does a guard value; deref coercion applies only to `&Guard`. So both choices the plan offers need an `&` added at those three sites, which the constraint and "Touch no file outside this task's files" forbid. The fourth outside caller, `backlog/check.rs:532` (`opts().limit`), would compile either way. The worker has to pick one of three things the plan has not settled: keep a `&'static` return, for example by leaking one boxed `OutputOpts` per in-process run; add those three files to task 6; or drop the constraint.

## False premises

1. **Task 7, Detail: "`TOMLCTL_LOCK_TIMEOUT` in resolve_lock_timeout (and its second read further down the file)".** There is no second production read. The other `std::env::var("TOMLCTL_LOCK_TIMEOUT")` is at `tomlctl/src/io.rs:2863`, in the test `tomlctl_lock_timeout_clamps_at_24h` inside `mod tests` (from line 2576). The second environment read the list does miss is in production: `ensure_process_root` at `tomlctl/src/io.rs:1936` reads `TOMLCTL_ROOT` through `std::env::var_os`. That read only picks the wording of the root-mismatch error. Under an in-process run whose overlay alone carries `TOMLCTL_ROOT`, it would print "TOMLCTL_ROOT is unset" when the variable is set. A worker following the list would edit the test read and leave the production one.

## Lesser observations (not counted)

- Task 2: the design says the not-a-regular-file case is "checked with the bash `-f` test before gawk runs". The constraint says "the order errors are reported in" stays. Today a missing file is reported in pair order, interleaved with the marker and drift errors (`scripts/verify-shared-blocks.sh:135-177`). Running every `-f` test and printing its errors before gawk starts would reorder them, so the worker has to pass the `-f` results into gawk and print them in sequence there. The constraint settles this, so it is not counted.
- Task 3: the hook's commands use `$ROOT/...` absolute manifest paths (`.githooks/pre-commit:66-92`), while the step table's commands are relative. That works, because git runs the hook at the top level and gate-stamp.sh `cd`s to the root. How the hook should run the string from `command <id>` (word splitting or an array) is left to the worker, and the commands contain no spaces.
- Task 7: `advise_fmt`'s `VISIBLE` cache (`io.rs:85-96`) is process-wide. "stays silent with a context" is enough instruction for a worker to handle it.

## Premise probes (all run at 4eeac3e0)

Every premise the eight tasks name holds:
- T1 `git_command(&root).args(["init", "-q", "."])` in common/mod.rs
- T2 `^has_marker()` and `^extract_block()`
- T3 `^ROOTS=(...)` and `^STAMP_STATE=unchecked`
- T4 `fn is_stdout_closed`
- T5 six `std::process::exit(` in the five files: count 6, at cli/dispatch.rs:396, plan/check.rs:50, tasks/dispatch.rs:326 and 356, review/anchor.rs:88, review/reproduce.rs:385. Each follows a flushing print, and each is inside a function that returns `Result`.
- T6 `^static OPTS: OnceLock<OutputOpts>`
- T7 `^static STDIN_CONSUMED`
- T8 `let cli = Cli::parse();` and `args_os` in cli/dispatch.rs

Other claims checked against the code:
- output.rs has exactly four `eprintln!` sites (909, 912, 1218, 1233).
- lib.rs emit_error has two stderr writes (`eprintln!` at 445, a `stderr().lock()` writer at 476).
- `configure` has the `debug_assert!` (291).
- `CLI_STACK_BYTES` exists (lib.rs:63).
- `check_where_placement` lives in output.rs:55 and is called from cli/dispatch.rs:157.
- `claude_dir` caches in a `OnceLock` (catalogue.rs:2197).
- `TOMLCTL_NO_DEDUP_ID` is read at items.rs:38.
- The output_gate test exempts only output.rs and test-only paths.

## Check commands

Forward checks, run on the present tree. Each fails here as it should before the change, so each one discriminates:
- T1 `grep -q 'CARGO_TARGET_TMPDIR' tomlctl/tests/common/mod.rs` → fails
- T2 `! grep -q 'has_marker "' scripts/verify-shared-blocks.sh` → fails
- T3 `grep -q 'plan-corpus' … && grep -q 'dispatch-tests' …` → fails; `grep -q 'gate fmt-tomlctl' .githooks/pre-commit && ! grep -q 'gate cargo' …` → fails; `grep -q '.claude/backlog.toml' scripts/gate-stamp.sh` → fails
- T3 `bash scripts/gate-stamp.sh check no-such-step; test $? -eq 2` → exit 1 today (check ignores the id and the digest does not match), so the test fails. It discriminates, but only while no matching stamp exists: with a matching stamp, base would exit 0, which still fails the test.
- C2 `bash scripts/gate-stamp.sh command fmt-tomlctl | grep -q 'cargo fmt'` → prints `usage: gate-stamp.sh run|check`, exit 2, and the grep fails
- T4 `test -f tomlctl/src/invocation.rs && …` and `grep -q 'struct ExitWith' …` → fail (file absent)
- T5 `! grep -rq 'process::exit' <five files>` → fails
- T6 `! grep -q 'eprintln!' tomlctl/src/output.rs` → fails; `grep -q 'invocation::' tomlctl/src/output.rs` → fails
- T7 the `invocation::stdin_reader` and `invocation::env_var_os` greps fail; their files do not reference it yet
- T8 `grep -q 'pub fn run_with' tomlctl/src/lib.rs` → fails; `! grep -q 'args_os' tomlctl/src/cli/dispatch.rs` → fails

Guard run by the worker:
- T2 `bash scripts/verify-shared-blocks.sh` → `shared-block parity: OK`, exit 0, real 0m33s. That fits the 24 s or more the plan cites as the cost being cut.

Not run, all marked "Writes build: yes" and left to the gate: the cargo nextest filters of T1, T2, T4, T5, T6 and T8. Every filter names a module that exists:
- `it.rs` declares `backlog_read`, `flow_active`, `plan_drift`, `plan_check`, `tasks_render`, `review_anchor`, `review_reproduce`, `blocks`, `output_options`, `output_trim` and `output_nesting`.
- `cli::dispatch::tests::skills` holds `both_gates_reject_a_cr_marker_and_an_empty_span`.
- `invocation::` and `in_process::` will exist once tasks 4 and 8 land.

T5's filter does not include the module that covers `tasks check` exiting 1, only `tasks_render`, so that verb's exit path is checked only by the full-suite gate.

## Coverage

Read: the worker views and file lists for tasks 1 to 8. The code at:
- tomlctl/tests/common/mod.rs
- tomlctl/tests/it.rs
- scripts/verify-shared-blocks.sh
- scripts/gate-stamp.sh
- .githooks/pre-commit (lines 40-94)
- tomlctl/src/lib.rs (run, emit_error)
- tomlctl/src/output.rs (OPTS, configure, opts, with_stdout, eprintln sites)
- tomlctl/src/io.rs (env, stdin and advisory sites)
- tomlctl/src/items.rs:38
- tomlctl/src/review/catalogue.rs:2197
- the six exit sites
- tomlctl/src/cli/dispatch/tests/output_gate.rs
- the opts() callers

Not checked: the nextest commands, which build; CLAUDE.md's Developer setup paragraphs that task 3 rewrites, beyond their content as loaded; and the D9 prototype timing, which lives in notes this cold read did not open.
