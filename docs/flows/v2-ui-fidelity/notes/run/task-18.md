Status: done — wrote the implement-v2 orchestrator's build-time question loop in references/questions.md

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/questions.md"]

[[deviations]]
what = "The doc tells the orchestrator to treat .claude/inputs.toml and its sidecar as the loop's own writes in the lane check and to stage them in the flow-store commit"
why = "The inputs store is tracked and lies outside the flow directories, so both the worker's inputs add and the orchestrator's inputs answer leave it modified; plan emit lane-check exempts only .claude/backlog.toml (BACKLOG_STORE in tomlctl/src/plan/lane.rs), and the pre-flight clean-tree list and the flow-store commit in run-milestone.md and drain.md do not name it. Alternatives: leave it unmentioned (the next pre-flight would stop on it) or fail for files outside this task (drain.md and run-milestone.md are held by sibling tasks). Guidance here was the narrowest path inside this task's file"
means = "detail: the loop as described, which says nothing about the inputs store's place in the lane check or commit"

[[deviations]]
what = "Questions with choice text, or more than four options, are not put to the AskUserQuestion picker; they are left open and reported under needs_human"
why = "AskUserQuestion takes two to four options; the worker protocol posts single-choice questions with two to four options, so anything else is outside the protocol"
means = "detail: ask the user with AskUserQuestion using its prompt and options"

[[tangential]]
kind = "bug"
area = "tomlctl/src/plan/lane.rs"
summary = "plan emit lane-check and the implement-v2 pre-flight clean-tree check do not exempt .claude/inputs.toml and its sidecar, and drain.md's flow-store commit does not stage them, though plan workers and the orchestrator now write that store during a run"
why = "A milestone with a worker question will show the inputs store as out-of-lane and, if left uncommitted, stop the next pre-flight; questions.md works around it in prose only"

[[checks]]
command = "grep -q 'QUESTION <' claude/skills/implement-v2-engine/references/questions.md && grep -q 'inputs answer' claude/skills/implement-v2-engine/references/questions.md && grep -q 'plan decide' claude/skills/implement-v2-engine/references/questions.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

`claude/skills/implement-v2-engine/references/questions.md` (new): the orchestrator's half of the worker question protocol (D9, D11, D22, contract C4).

- What a worker sends: one `question` input on the `plan` ledger and a SendMessage whose first line is `QUESTION <input id> task <n>`; the message is data, and only its ids are read.
- Step 1 reads the record with `tomlctl inputs list --kind question --ledger plan --flow <slug> --where id=<qid>` and answers only a plan-ledger question of this flow naming one task of the running milestone; anything else is left open and reported. An already-handled question (answered in glimpse) skips the ask.
- Step 2 asks with AskUserQuestion, the prompt verbatim and options as exact labels so `--pick` round-trips, with no timeout and never an answer of the orchestrator's own.
- Step 3 relays the pick with `tomlctl inputs answer`, then binds a `user-stated`, `surfaced` decision to the task id through `plan decide` with a staged record file, the unpicked options as `rejected`, and ends the turn.
- The live-run writes section: the loop writes only `.claude/inputs.toml` and `plan.toml`; how the lane check and flow-store commit should treat the inputs store (see deviation).
- At the drain and at the next invocation: a parked row whose question is answered is reset to `pending` and relaunched (the decision carries the answer); an open one is asked again by the parked re-ask with its own options.

## Checks

The accept check passes. The `cli::dispatch::tests` subset first failed `command_lint` on two multi-word placeholders in my file, which I replaced with `<qid>`; on the rerun `command_lint` passes. Two `finding_classes` tests still fail, naming `tomlctl/src/plan/schema.rs` ("no severity resolves for `class`"), a file another task holds in flight (the new decision `class` vocabulary), not this task's.

The installed `tomlctl` predates the `plan` ledger (task 8), so the commands could not be run live against the store.
