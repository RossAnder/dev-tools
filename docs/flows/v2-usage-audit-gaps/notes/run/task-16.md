Status: done — drain step 7 reruns failures at base, routes base failures to the user, mints worker-run reproducers, checks fix rows against decisions and narrows after a whitespace round

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/drain.md"]

[[deviations]]
what = "The base-rerun record call writes its flags as `--base --blocks <path>`, not in the `--blocks <path> --base` order plan-run.md prints"
why = "The accept check greps the literal `--base --blocks`; clap takes the flags in either order, and the installed binary's help lists both"
means = "detail (1): record with `plan run gate ... --base --blocks <file>`"

[[deviations]]
what = "A backlog-routed base failure is found in later rounds by a fixed summary prefix (`base failure in <slug> milestone <id>: `) read with `backlog list --grep`, and a waived one by the milestone's `kind = base` gate_accept records"
why = "The spec says a routed base failure is not asked about again but names no record for the backlog route; the backlog item is the only durable trace, and a fixed prefix avoids grepping for test ids full of regex metacharacters"
means = "detail (2) and the carry constraint"

[[deviations]]
what = "The base call's verification items carry no `rerun:` option, only the gate's `timeout:` and `transient:`"
why = "Each item is already the narrow rerun of the failed ids; a second narrow rerun at base would turn a load flake into a base pass and mint a fix for a failure that predates the milestone"
means = "detail (1)"

[[tangential]]
kind = "debt"
area = "claude/skills/implement-v2-engine/references/drain.md"
summary = "drain.md is now 584 lines against the 600-line reference ceiling, leaving task 17 (gate waiver, stopped-gate cleanup, source promotion) about 16 lines before it must move overflow into another references file"
why = "skill_references_under_line_ceiling fails the gate past 600; task 17's edits to the same file will likely need a split"

[[checks]]
command = "grep -q -- '--base --blocks' claude/skills/implement-v2-engine/references/drain.md && grep -q -- '--after-round' claude/skills/implement-v2-engine/references/drain.md"
result = "pass"

[[checks]]
command = "grep -q 'run_by = \"worker\"' claude/skills/implement-v2-engine/references/drain.md"
result = "pass"

[[checks]]
command = "tomlctl plan run gate --help; tomlctl plan emit gate-list --help; tomlctl backlog list --help (flag names checked against the installed binary)"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

`claude/skills/implement-v2-engine/references/drain.md`:

- Step 6, New tasks: the example fix row's accept now carries `writes_build = true` and `run_by = "worker"`, and the ownership sentence names that exception. A worker runs such an entry even when it builds, and `plan emit launch-args` marks its task `builds`, which the pool runs one at a time.
- Step 6 intro: a gate fix row that departs from a decision asks the user before launch, by step 7's decision check, and is not parked.
- Step 7, options: `rerun:` goes under every test command whose runner takes ids, in the form `claude/agents/verification.md` documents for each runner. The text links that file and does not repeat the forms, so a timeout under load comes back `flaky`.
- Step 7, pass rule: a run whose only failures are routed base failures passes the gate.
- Step 7, Fix rounds is rewritten as three subsections:
  - **Rerun at base.** Make a detached worktree of HEAD and prepare it with dependency links and the untracked or ignored env files. Dispatch one verification call over the latest run's `fail` commands in order, as `cd <wt> && <rerun with ids>` or as the whole command. Held commands and forward accept checks are left out and take `skip:` lines. A whole-command rerun that fails before reaching its tests is also replaced by a `skip:` line, so it counts as the milestone's own failure. Record with `plan run gate --base --blocks`, then remove the worktree.
  - **Base failures go to the user.** Failures that predate the milestone and are already routed come from the `kind = base` gate_accept records and the base-failure backlog items. Every other base failure goes to the user through AskUserQuestion: fix it under a `base <id>: ` reason, backlog it, or waive it with a `--accept --json` payload of kind `base`. A base record does not by itself let the milestone commit. The first run whose only failures are routed base failures records one kind `accept` acceptance and commits.
  - **Mint the milestone's own failures.** Reproducers are `run_by = "worker"`. The carry-forward rule is unchanged. A new decision check reads `plan view --for worker --task <n>` for each fix row; when a row departs from a decision, the user chooses between amending it (`plan decide` on the same id, user-confirmed, binds re-issued with the row) and reshaping the fix. Gating again uses `plan emit gate-list --after-round <n>`. When `narrowed` is true, the gate runs only the commands still owed and names the carried ones in the report's gate summary.
- Two rounds at most: `base <id>: ` rows are not counted, and a gate that fails after round 2 only on routed base failures is not stopped.

I set the row `in-progress` after the edits instead of before them. Nothing else wrote to the row in between.

The `cli::dispatch::tests` subset is marked `Run by: gate` in my view, so I did not run it. I checked the verb and flag names against the installed binary's `--help` instead.
