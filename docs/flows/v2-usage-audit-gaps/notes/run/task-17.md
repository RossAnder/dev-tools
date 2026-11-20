Status: done — the drain asks past the last gate round, kills a stopped gate's tree by its cN.pid, and promotes run-note sources into docs/references/

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/drain.md"]

[[deviations]]
what = "Gave each gate dispatch a run_dir of <scratch>/gate-<id>-<n>-<k>, k counting the round's dispatches from 1, rather than one directory per round"
why = "A round can hold several verification dispatches (the first call, held commands, a timeout rerun, the base rerun, a re-dispatch after a stop); sharing one directory would let a later dispatch overwrite the cN files a stopped one left"
means = "dispatch every gate with a run_dir: in the scratchpad"

[[deviations]]
what = "Wrote the reference entry format and the extra-round cap read inline in prose, with no example block"
why = "drain.md stood at 584 lines before this task and the skill reference ceiling is 600; inline text kept the file at 596 without moving content to another file, which this task does not hold and D10 argues against for step 7"
means = "Move overflow into a further references file linked from the body"

[[tangential]]
kind = "question"
area = "claude/skills/implement-v2-engine/references/drain.md"
summary = "drain.md is at 596 of its 600-line ceiling"
why = "The next prose addition to the drain will trip skill_references_under_line_ceiling and need a section moved out first"

[[checks]]
command = "grep -q 'extra-round' claude/skills/implement-v2-engine/references/drain.md && grep -q 'cN.pid' claude/skills/implement-v2-engine/references/drain.md && grep -q 'docs/references/' claude/skills/implement-v2-engine/references/drain.md"
result = "pass"

[[checks]]
command = "wc -l claude/skills/implement-v2-engine/references/drain.md"
result = "pass"
```

## What changed

`claude/skills/implement-v2-engine/references/drain.md`, building on the uncommitted base-rerun and decision-check edits already in the file:

- Step 7, dispatch: every gate dispatch (first call, held commands, base rerun, re-dispatch) passes `run_dir: <scratch>/gate-<id>-<n>-<k>`, shown in the example input, matching the `run_dir:` input verification.md now documents.
- Step 7, "A stopped gate agent": for each `cN.pid` without `cN.exit`, kill the tree by pid (`taskkill //PID <pid> //T //F` on Windows, `kill -TERM -<pid>` on POSIX, `kill -TERM <pid>` for a launch without `setsid`), confirm it is gone, leave project locks alone, add one `reconcile` report entry, and re-dispatch under the next `<k>`; a tree that will not die goes to `needs_human`.
- Step 7, "Two rounds at most": the cap is two plus one per `extra-round` `[[gate_accept]]` row; past it, "Past the last round" settles any timeout first, then asks accept and commit (debt backlog item first, then `plan run gate --accept --json` of kind `accept` naming every failing command), one more fix round (kind `extra-round`, then mint), or leave uncommitted (the old path).
- Step 8, "Sources outlive the flow": reads attempt `sources`, promotes each into `docs/references/<topic>.md` (topic = the service or library, lowercased and hyphenated, one file per topic; one `##` heading per value carrying url or query, how_verified, fetched_at, version and `from: <slug> task <n>`), dedupes by url or query and claim, and names the drain the only writer.
- Step 10: the commit condition accepts a `[[gate_accept]]` row of kind `accept` (never a `base` row alone); the flow-store commit stages each promoted reference file explicitly beside `plan emit commit-paths --for flow`'s printed paths, which never include `docs/references/`. Stamping on re-entry reads the gate acceptance for the last gate round and never re-gates it; Next names a gate the user left uncommitted.

The file is 596 lines by `wc -l`, under the 600 ceiling. The `cli::dispatch::tests` subset is marked `Run by: gate`, so it was not run here.
