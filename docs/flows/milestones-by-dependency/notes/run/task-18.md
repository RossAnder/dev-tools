Status: done — wrote plan-v2-engine/references/elaborate.md, the elaboration procedure that flips a deferred milestone only after its cold read and probes

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/references/elaborate.md"]

[[deviations]]
what = "The procedure states the two concurrency rules (a concurrent flow-store commit sweeping rows in is safe; the cold read excludes the running milestones' files) without naming them 'user decision D1' and 'D2'."
why = "The repo's documentation rule keeps plan record ids out of shipped skill text, and a decision id of this flow means nothing to a reader of a generic reference."
means = "detail: 'Say that ... (user decision D1)' and '(user decision D2)'"

[[deviations]]
what = "The update-record example carries a `ref` row key beside the `title` under `[record.set]`."
why = "tasks update refuses a new title without its new ref (write-and-prove.md, Retitling a row); an example showing title alone would be refused."
means = "detail: show one record with action, detail, title and checkpoint under [record.set], spec top-level and reason"

[[deviations]]
what = "Added a section 7 on re-entry and open runs: own-session resume, cold-read:<id> relaunch with HEAD allowed to move when the fan-out check with the launch's excludes passes, and another session's run asked about before any abandon."
why = "The detail's abandon rule needed a home, and write-and-prove's relaunch rule requires HEAD equality, which a concurrent implement-v2 commit breaks."
means = "detail step list (1)-(6)"

[[tangential]]
kind = "question"
area = "claude/skills/plan-v2-engine/references/write-and-prove.md"
summary = "The cold-read Relaunch rule requires HEAD to equal head-before.txt; with --exclude the fan-out check tolerates HEAD moves through excluded paths, so the two disagree for any milestone-scoped cold read."
why = "elaborate.md relaxes it locally; if write-and-prove is ever used for a cold read beside a live run, the stricter rule will force needless abandons."

[[checks]]
command = "accept 1: test -f elaborate.md && grep -qF 'elaborate:<id>' && awk cold-read:<id> before milestone update --elaborated"
result = "pass"

[[checks]]
command = "accept 2: grep -qF -- '--exclude' elaborate.md"
result = "pass"

[[checks]]
command = "contract C4: grep -q -- '--exclude' fanout-check.sh && grep -q -- '--exclude' elaborate.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass (56 passed)"
```

## What changed

`claude/skills/plan-v2-engine/references/elaborate.md` (new, 249 lines): the procedure for `/plan-v2 <slug> --elaborate <id>`.

1. Preconditions: `flow doctor`, `plan stage` (approved or running), `plan emit schedule` with a state table (`deferred` continues, `elaborating` is a resume or an ask, anything else refused), a non-empty `on` put to the user, `plan run start --step elaborate:<id>` before any write, and reading the defer reason, rows and what upstream commits built.
2. Surface and decide by open-and-challenge.md section 5, one AskUserQuestion, `plan decide`, design files only where needed, gate changes through `plan milestone update` with a reason.
3. Revise rows: `add-many` with needs only on existing ids of this or earlier-stored milestones (a later-stored need stops and goes to the user), a `tasks update --ndjson` record example with action/detail/title/checkpoint under `[record.set]`, `spec` and `reason` as row keys, then edges, binds, contract tasks and the 4a renamed-names sweep.
4. Prove while deferred: `plan check --milestone`, premises and probes with `--milestone`, the cold read as step `cold-read:<id>` with the excludes (every row file of milestones not done/empty/deferred/elaborating, `notes/run`, finding-link review stores) passed as `--exclude` to both snapshot and check, gate probes.
5. Flip with `plan milestone update --elaborated` as the last tasks.toml write, then `plan check --milestone` again.
6. `plan run finish --step elaborate:<id>`, one commit staging paths by name, the note that a concurrent flow-store commit sweeping rows in is safe, and a close listing agent-assumed decisions.
7. Re-entry and open runs.

Wiring the reference into SKILL.md and the carrier is a sibling task's.
