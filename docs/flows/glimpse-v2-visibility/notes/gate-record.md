Verdict: None of the three is stored as a record. Per-command gate results exist only in the verification agent's reply and a throwaway `mktemp -d` log (claude/agents/verification.md:40). The gate has no run step (claude/skills/implement-v2-engine/SKILL.md:57). Fix rounds leave a trace only as `gate <id> round <n>:` reason prefixes on `[[changes]]` rows in tasks.toml (drain.md:322, tomlctl/src/tasks/add.rs:422). `needs_human` is printed text that nothing persists (drain.md:424).

## Answer

- **Gate results (outcome, duration, log path):** not stored. The `verification` agent returns `outcome`, `duration`/`duration_s` and `log` per command, but only in its reply to the orchestrator (claude/agents/verification.md:149-159). The log sits in a `mktemp -d` scratch directory in system temp, outside the repo and the flow (verification.md:40, :77). The engine records no run for the gate, so `plan.toml` `[[run]]` holds no gate row (SKILL.md:44, :57). drain.md step 7 writes nothing either: a pass goes straight to the commit, and a fail mints fix tasks (drain.md:304-317).
- **Fix rounds:** stored only indirectly. Each fix task is minted through `tasks add-many` with a `reason` that opens `gate <id> round <n>: <command and ids>` (drain.md:322). Because v2 stores are audited, `add-many` writes a `[[changes]]` row with `field = "*"`, `new = title` and that `reason` (tomlctl/src/tasks/add.rs:422-434; Change struct at tomlctl/src/tasks/schema.rs:395). The engine counts spent rounds by prefix-matching those reasons (drain.md:331). A round that passed, the outcome of the gate that triggered a round, a `timeout` re-dispatch and a gate that passed first time all leave nothing. Reproduce-check rounds use the same scheme with the prefix `reproduce <id> round <n>:` (apply-rows.md:152). The reproduce check does record a `reproduce:<id>` run and `[[reproductions]]` in the review store, but that is not the gate.
- **`needs_human` entries:** not stored. The report is assembled at drain.md step 11 and emitted as conversation text (drain.md:424-465). Some of its inputs are durable on their own: parked and failed rows (tasks.toml `status`), undone handovers (plan.toml, through `plan handover set`, drain.md:366), and abandoned runs (plan.toml `[[run]]` `reason`). Several of its sections are durable nowhere: `gate:` (failed after round 2, timed out twice, covered_by skips), `audit:`, `out of lane:`, `store changes:`, `reconcile:`, `withheld:` and the stop reason.

## Findings

- **Library/API or file**: claude/agents/verification.md:40 (Step 0 `D=$(mktemp -d)`), :149-159 (Output fields)
- **Source**: file read. Real flow check: `ls docs/flows/v2-apply/notes/run` holds only `task-*` notes, and `.claude/flows/v2-apply/` holds no gate artifact.
- **Evidence-grade**: high
- **Finding**: The per-command outcome, duration and log path exist only in the verification agent's return and a temp log outside the tree. No store or file keeps them.
- **Details**: Logs go to `<D>/cN.log` under a system temp directory, and the agent writes nowhere else ("`<D>` is the only place you write", verification.md:67). The orchestrator reads the blocks (drain.md:304-313) and keeps none of them. A gate history view therefore needs a new durable record, such as a gate step run carrying per-command results, a gate note, or a task-store or plan-store array, plus a writer at drain.md step 7. The log is also not guaranteed to survive.
- **Counter**: A store field or note path for gate results that I missed, found by grep for `duration_s` or `outcome` in tomlctl/src/plan or the engine references, would refute this. Neither grep found one.
- **Impact on plan / report**: Gate history needs a new contract: a recorder verb or note shape, and a drain step that writes it. Logs need copying into the flow, or the view can only show a path that may be gone. Possibly `needs-plan`.

- **Library/API or file**: claude/skills/implement-v2-engine/SKILL.md:57 (Gate row, Run step `—`); tomlctl/src/plan/schema.rs:248 `Run`
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: The gate records no `plan run` step. The `[[run]]` timeline shows `implement:<id>` and `reproduce:<id>` and never a gate.
- **Details**: `Run` carries step, started, finished, outcome, reason and tasks, and nothing per command. The only run steps the engine records are `implement:<id>` (run-milestone.md:220) and `reproduce:<id>` (apply-rows.md:103).
- **Counter**: A `--step gate:<id>` call anywhere in the engine references would refute this. A grep of `--step` across claude/skills/implement-v2-engine found none.
- **Impact on plan / report**: A gate run step is a candidate home for gate timing, but `Run` has no per-command fields, so a step row alone would give start and finish but not command outcomes.

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:322, :331; tomlctl/src/tasks/add.rs:422-434
- **Source**: file read. `tomlctl items list .claude/flows/v2-apply/tasks.toml --array changes --where-prefix "reason=gate " --select ref,reason --lines` printed five `gate A round 1: …` rows.
- **Evidence-grade**: high
- **Finding**: Fix rounds are recoverable only by parsing the `gate <id> round <n>:` prefix of `[[changes]]` reasons on the minted fix rows. The rows tie each round to its fix tasks and give a timestamp (`at`).
- **Details**: The reason is free text written by the orchestrator: the command plus the failing ids and a cause. A round's start, its gate outcome and its duration are not recorded. `carried from gate <id> round <n>:` reasons (drain.md:324) mark constraint carries, and a parser must tell them apart from rounds.
- **Counter**: If `add-many` on an audited store did not write a `[[changes]]` row, this would be wrong, but the v2-apply query output above shows it does.
- **Impact on plan / report**: A view can derive fix rounds per milestone today from `[[changes]]` plus the rows' `checkpoint`, with no new contract. A structured field would make it robust.

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:424-465 (step 11, the `needs_human` report)
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: The `needs_human` report is conversation output only. Of its sections, only the parked, failed and handover entries are durable, and they are durable through their own stores, not through the report.
- **Details**: Its inputs are gathered from `plan view --for digest` `open_items` and `tasks list --where status=parked|failed` (drain.md:429-431), which glimpse could read directly. The `gate`, `audit`, `out of lane`, `store changes`, `reconcile` and `withheld` sections and the stop reason are composed in the conversation and lost when it ends.
- **Counter**: A write of the report to a file or store would refute this. None appears in drain.md step 11 or SKILL.md.
- **Impact on plan / report**: The Needs-you queue can show parked tasks and open handovers from existing stores. The remaining `needs_human` entries need a new persisted record written at step 11.

Coverage: read in full verification.md lines 1-99 and 149-187, drain.md lines 270-468 plus grep across the engine references, SKILL.md lines 40-70, tomlctl/src/plan/schema.rs `Run`, tomlctl/src/tasks/schema.rs `Change`, and tomlctl/src/tasks/add.rs's audited push. Checks run: a grep for gate, needs_human and verification across glimpse/src/model.rs (no match), a listing of the v2-apply flow's directories, and a `[[changes]]` prefix query. Ruled out: execution-record.toml, which implement-v2-engine never writes (grep found no hit), and agents.toml, which may record that a verification agent ran but not command outcomes (the agent-metrics lens owns it).
