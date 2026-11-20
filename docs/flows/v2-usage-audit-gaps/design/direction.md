# Direction

## Overarching

Close each audited cause where the engine already has a seam for it: the gate-round record, the visual-accept pattern, the accept-owner vocabulary, decision binds, the handover record, the run-note block and the worker procedure block. Build no new subsystem and rework nothing implement-v2-run-hardening delivered.

The work optimises for two things. A consumer milestone run should spend no fix round, user question or wrong turn on these causes. Every exception the user grants should leave a record that a later invocation, the report and the backlog can read. To get there, it gives up some speed in planning, through baseline gate runs and a recheck of late plan edits, and in fix rounds, where building reproducers run one at a time.

Expectations every task keeps:

- tomlctl never runs a stored command. Anything that must run (a base rerun, a baseline gate, a handover `ready_check`) is run by the orchestrator through the `verification` agent or its own shell, and tomlctl only records the result.
- A new store shape is the one described in [records.md](records.md), so parallel tasks agree on it; a task that needs a shape not described there stops and reports it rather than inventing one.
- Prose rules are written for consumer repos, which load no dev-tools CLAUDE.md: a rule an agent must follow lives in the agent, skill or reference it reads.
- Each exception the user grants (a gate waiver, an extra round, an inspector disagreement, a skipped recheck) is asked through `AskUserQuestion` with the default first, recorded, and printed in the report.

## Milestone A

Milestone A is the binary. It adds every tomlctl record, field, flag, check class and feature name the prose of milestone B uses, with tests and the `tomlctl` skill's flag references, then installs the binary as a held external gate. A handover after A's commit relinks glimpse (`cargo install --path glimpse --locked` with its pane closed), since glimpse loads task stores through tomlctl's reader, which refuses the new task-store version. It writes no engine, agent, Workflow or carrier text, so this run's own drain reads unchanged engine text while A's workers edit tomlctl.

Its tasks read [records.md](records.md) as the contract, keep each verb's existing behaviour unchanged when the new flag or field is absent, and refuse a malformed payload with a message that names the field.

## Milestone B

Milestone B is the prose, after A. It teaches the engines, agents, the Workflow scripts, the skills and both carriers to use what A installed:

- the drain's base rerun, the narrowing after a whitespace-only round, the gate waiver and the stopped-gate cleanup;
- the decision check on fix rows;
- the serial build slot and worker reproducers;
- baseline gates and the late-edit recheck;
- blocking handovers and the done-mark commit;
- the reconcile step in elaboration;
- the no-writes window;
- the sourcing rules and the reference store;
- the visual disagreement route and the capture-config question;
- the self-edit rule.

Each rule names the verb and flag it runs exactly as milestone A shipped them, so the dispatch gates' flag-drift tests pass. This run applies the self-edit rule to itself by hand: before B's first launch the orchestrator copies `claude/skills/implement-v2-engine/` and the saved Workflow scripts under `claude/workflows/` into the scratchpad, launching B's rounds from the copied `implement-v2-milestone` script, and drains, gates and runs B's fix rounds from that copy with the pre-B verification contract, since B rewrites the drain and the verification agent this session loaded; the new rules take effect from the next session. A rule's text stays in the one file that owns it, and other files cite it rather than restating it.
