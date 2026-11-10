Verdict: Milestone B cannot be carried out exactly as written. Three points would leave a worker guessing (one each in tasks 14, 15 and 16), and one false premise means task 14's `node --check` acceptance can never pass.

# Cold read: milestone B (tasks 14, 15, 16)

## Unactionable

### U1. Task 14: the plan never settles what each reproduce verifier returns, or which arg selects the mode
- "the mode runs one `review-verifier` per unit ... and returns one entry per unit." The plan does not say what an entry holds, or what schema the `agent()` call for each verifier is given. Every dispatch today passes a schema taken from args (`schemas.verdict_return`, `claude/workflows/review-v2.js:43-44,107`). That schema describes the verdicts `confirmed|plausible|refuted` (`claude/agents/review-verifier.md:45`), and it does not fit `gone|reproduces|unsettled`. The worker has to decide three things alone: whether the reproduce args carry a new `schemas.recheck_return` or the script inlines one, what a unit's entry looks like, and how a missing return is recorded in `errors`. Task 16 then has to match whatever the worker picked.
- The plan also names no arg for the mode itself. "args carry the store, flow, milestone, round and one unit per apply task" gives no key that tells the script to use reproduce mode, and no key for the run directory. At present the run directory is derived from `pack_path` (`review-v2.js:50`), and reproduce mode has no pack. There is a precedent the worker could copy, `mode: "recheck"` and `run_dir` in the plan-v2-review wrapper (`claude/skills/review-v2-engine/references/plan-flow.md:316-318`), but the task does not point to it. This half can be guessed. The return shape cannot.

### U2. Task 15: the verifier's shared store-writer block forbids the new write, and that block must stay byte for byte
- The constraints say "Leave every shared block in review-verifier.md byte for byte." The detail says the verifier records its answers "written with `tomlctl review recheck`".
- The `review-v2-store-writer` shared block in `claude/agents/review-verifier.md:50` says: "Your only other writes are `tomlctl review` calls ... — `add`, `cover` and `verdict` for review agents". `recheck` is not in that list. The block is held identical across seven agents (`scripts/shared-blocks.toml`, block `review-v2-store-writer`), and all of them sit outside this task's files.
- The worker has two options and the plan picks neither: leave the agent's body contradicting its own write contract, or break the constraint by editing the block in all seven carriers. The same gap applies to the projection. Design step 3 has the verifier read `--for implementer`, but `review-verifier.md:22` says "never another role's projection", and the task's Detail does not mention the exception.
- The flags of the verb are not in the worker view either. Task 15 has no `Needs` on task 5, and its text never says `--ndjson`. The worker has to find the interface from task 5's code or from `--help` on a reinstalled binary. `tomlctl review --help` on the installed binary today lists no `recheck` verb.

### U3. Task 16: the SKILL.md edits it names do not cover the lines the design contradicts
- The detail says "SKILL.md's rule that workers are the only Workflow launch and its line that there is no vet pass both change". No rule says workers are the only Workflow launch. Rule 3 (`claude/skills/implement-v2-engine/SKILL.md:26`) says workers run only through `implement-v2-milestone` and that the engine never falls back to the Agent tool. The "no vet pass" line is `SKILL.md:73`.
- The design's last bullet requires "With no Workflow tool the units run as an Agent wave, one `review-verifier` each." That contradicts two lines the task does not mention. Rule 4 (`SKILL.md:27`) reads "The `verification` agent is the one Agent dispatch". Out of scope (`SKILL.md:75`) reads "An Agent-tool fallback when the Workflow is unavailable or refuses a launch". The worker has to decide whether to rewrite these, which the plan neither asks for nor forbids.
- Smaller gap in the same task: for `unsettled`, the design says "when it cannot tell, parks the row". At that point every apply row is `done`, and the plan does not say which row is parked: the done apply row, or a newly minted fix row.

## False premises

### F1. Task 14's guard "the script parses: `node --check claude/workflows/review-v2.js`"
- Workflow scripts use a top-level `return`, and `node --check` rejects that on the present tree: `SyntaxError: Illegal return statement` at `review-v2.js:47`. The repo records this itself, in `tomlctl/src/cli/dispatch/tests/lint.rs:1036`: "It validates no JS syntax, because a script's top-level `return` fails `node --check`." The check fails before the change and after it, so it cannot tell a good edit from a bad one. The closest working gate is the Workflow-script catalogue test in `cli::dispatch::tests`.

## Check commands (run on the present tree)

| Task | Command | Output | Reading |
|---|---|---|---|
| 14 premise | `grep -q 'SCRIPT_VERSION' claude/workflows/review-v2.js` | rc=0 | holds (`review-v2.js:19`, value `'2'`) |
| 14 forward | `grep -q 'review recheck' claude/workflows/review-v2.js` | rc=1 | discriminates |
| 14 guard | `node --check claude/workflows/review-v2.js` | rc=1, `SyntaxError: Illegal return statement` (line 47) | fails on the present tree for an unrelated reason; see F1 |
| 15 forward | `grep -q 'review recheck' claude/agents/review-verifier.md` | rc=1 | discriminates |
| 15 forward | `grep -qi 'reproduce' claude/skills/review-v2-engine/references/agent-waves.md` | rc=1 | discriminates |
| 15 guard | `bash scripts/verify-shared-blocks.sh` | `shared-block parity: OK`, rc=0 | valid guard |
| 16 premise | `test -f claude/skills/implement-v2-engine/references/findings.md` | rc=1 | expected: the file arrives with task 12 (premise is `after:12`) |
| 16 forward | `grep -q 'review recheck-status' .../findings.md && grep -q 'reproduce <id> round' .../findings.md` | not run usefully, file absent | will discriminate once task 12 lands |
| 16 forward | `grep -qi 'reproduce' claude/skills/implement-v2-engine/SKILL.md` | rc=1 | discriminates, but weakly: one word anywhere passes |

Other checks:
- `tomlctl plan run start --step` takes a free-form step name, so `reproduce:<id>` is accepted.
- `review show ... --for implementer` exists (`--for` lists `implementer`; `Role::Implementer` in `tomlctl/src/review/fields.rs:100`).
- Task 5 specifies `recheck-status <ids> --expect <verdict>`, which answers by exit code only. Task 16's "read the result with `review recheck-status`" and its per-verdict table therefore mean one call per finding per verdict. A worker can work this out, but the task does not say it.
- drain.md step 6's hand-over to step 7 is at `claude/skills/implement-v2-engine/references/drain.md:214`, as task 16 assumes.

## Coverage

- **Read:** the worker views and the `files` lists of tasks 14, 15 and 16; `claude/workflows/review-v2.js` (in full); `claude/agents/review-verifier.md` (in full); the heading outlines of `verifying.md` and `agent-waves.md`; `implement-v2-engine/SKILL.md:14-83`; `drain.md:200-229`; `scripts/shared-blocks.toml` (store-writer block); `lint.rs:1033-1037`. For their files, milestones and interfaces only: the task 5 spec section and task 12's file list.
- **Not checked:** the `recheck` and `recheck-status` interfaces against code, since task 5 has not landed; `findings.md`, since task 12 has not landed; and whether the Workflow-script catalogue test accepts a new `mode` key.
