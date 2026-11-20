Status: done — verification takes run_dir:, writes cN.pid at each detached launch, and gains {ids_regex} with bun and vitest rerun forms

```toml run-note
files_touched = ["claude/agents/verification.md"]

[[deviations]]
what = "Step 1b documents how to stop the tree that cN.pid names: under MSYS a walk over MSYS ps descendants of the row whose WINPID is in the file, taskkilling each WINPID with //T; under POSIX pkill -TERM -s <id> rather than kill -TERM -<pgid>"
why = "Probed on this machine: taskkill //T on /proc/$!/winpid stopped only that bash, because every MSYS fork leaves the exec'd child's Windows parent dead (timeout.exe and the c1.sh bash had dead Windows parents), so the native tree was unreachable. The ps walk stopped the whole tree including a native cmd/ping grandchild. On POSIX, GNU timeout calls setpgid itself (MSYS ps showed the timeout process leading its own PGID), so a kill of the launch's process group misses the command; the session id setsid gives covers it. The file still holds the winpid and the setsid process group id the detail names; only the stop recipe beside it differs from what D13 assumed."
means = "detail: the Windows pid from /proc/$!/winpid, which taskkill can reach; on POSIX the process group id"

[[deviations]]
what = "{ids_regex} turns each id's ' > ' separators into single spaces before escaping"
why = "bun test -t matches against the test name prefixed by its describe labels joined with spaces (Context7, oven-sh/bun docs/test/discovery.mdx), while bun's (fail) lines, which the extractor records, join them with ' > '; without the translation the bun rerun would match nothing"
means = "detail: each id regex-escaped, joined with |"

[[deviations]]
what = "Without setsid and outside MSYS, as on stock macOS, no cN.pid is written and the launch check needs only cN.log"
why = "Writing $$ from a non-session-leader would hand the caller the agent's own session or group to kill"
means = "detail: on POSIX launch under setsid and write the process group id"

[[deviations]]
what = "The regex escaper is an awk helper written in Step 0 (ids_regex.sh) that builds the backslash with sprintf(\"%c\",92)"
why = "Through the Bash tool a doubled backslash inside single quotes reached sed as one (s/.../\\\\&/ printed a bare &), and \\x5c in a sed replacement is GNU-only; the awk form passed a match test on ids holding ( ) . [ ] * | \\ $ ^ + ? { }"
means = "none; the detail leaves the escaping mechanism open"

[[tangential]]
kind = "bug"
area = "claude/skills/implement-v2-engine/references/drain.md"
summary = "The stopped-gate drain row (task 17) must not kill a cN.pid with taskkill //T alone or kill -TERM -<pgid>; neither reaches the gate's tree"
why = "D13's text implies those forms; verification.md's Step 1b now holds the recipes that work (MSYS ps descendant walk, pkill -s), which the drain row should cite rather than restate"

[[checks]]
command = "grep -q 'run_dir:' claude/agents/verification.md && grep -q 'cN.pid' claude/agents/verification.md && grep -q 'winpid' claude/agents/verification.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

`claude/agents/verification.md`:

- **Input.** A new optional `run_dir:` names the directory to use as `<D>`. `rerun:` now documents `{ids_regex}` beside `{ids}`. A command or rerun may carry its own `cd <dir> && ` prefix, which runs in that command's own shell. The example adds a bun form (`cd web && bun test -t {ids_regex}`) and a vitest form (`cd spa && npx vitest run {ids}`), and a short paragraph says why each runner takes the form it does.
- **Step 0.** With `run_dir:`, the first line becomes `D='<run_dir>' && mkdir -p "$D"`, and the rest is unchanged. Step 0 also writes `ids_regex.sh`.
- **Step 1b.** There are three launch branches:
  - MSYS: nohup, then `cat /proc/$!/winpid > cN.pid`.
  - POSIX with setsid: the session leader writes its own `$$`.
  - Neither: no pid file.
  
  A block explains what `cN.pid` holds on each platform and how a caller stops the tree it names, with the tested MSYS ps-walk command.
- **Step 3.4.2.** `{ids_regex}` is filled from the helper's single output line, in single quotes, with the same skip on an id containing `'`.
- **Hard rules.** The `cd` sentence is added. The agent still interprets nothing.

The pid and stop behaviour was probed live under Git Bash with a detached `timeout 90 bash c1.sh` that ran `cmd //c ping`. `taskkill //T` on the winpid alone left timeout.exe and the c1.sh bash running. The ps walk stopped every process, including PING.EXE. All test processes were stopped by pid or exited on their own. The POSIX setsid branch and `pkill -s` could not be run on this Windows machine. GNU timeout's own process group was seen in MSYS ps, where PGID equals the timeout pid.
