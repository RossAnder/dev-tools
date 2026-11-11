Status: done — drain.md builds the gate from `plan emit gate-list`, audits the launch's agents, widens a row from `needs_files` and reads uncommitted paths unquoted

## What changed

`claude/skills/implement-v2-engine/references/drain.md`:

- Step 2: the index shape lists `needs_files` and says what it holds; the `failed` row of the status table points a non-empty `needs_files` at the new widening subsection.
- Step 3: the opening sentence no longer says drift must run before status moves; it states that a row with a set `agent` and no `commit` is own work whatever its status. The out-of-lane bullet loses the sentence about `plan drift` counting only rows past `pending`; an `uncommitted` array now means a file of a row no worker has started.
- Step 3, new subsection "What the run's agents ran": `tomlctl agents audit --transcripts <dir>` over the directory from the launch result's `Transcript dir:` line, kept in `<scratch>` by the launch step. A row is allowed when its command equals or narrows an accept check or a detail-named check of any row of the milestone, or is the `cli::dispatch::tests` subset; every other row is a breach. Every row is listed under a new `audit:` section of the step 11 report, with agent id, command and verdict. No status or row changes.
- Step 6: the replan table splits `failed` into "with `needs_files`" and "any other". New subsection "Widening a row from `needs_files`": an unjustified path parks the row for the user. With no pending or started holder, one audited row adds the paths to `files`, resets the row to `pending` and quotes the note in its `reason`. With a holder, an existing dependency path in either direction (`tasks closure --up`/`--down`) is kept; otherwise the widened row needs the holder through `set.needs`. There is a worked TOML record.
- Step 6, "Edits follow their files": the `git status --porcelain -- <path>` read is replaced by `tomlctl plan drift --slug <slug> --uncommitted --get uncommitted_files`.
- Step 7: the milestones read, the accept read and the by-hand filter and de-duplication are replaced by `tomlctl plan emit gate-list`. `skipped` entries are not run, and each `covered_by` entry is reported in a new line of the report's `gate:` section. The held-command rule and the `timeout:`/`rerun:`/`transient:` options are stated as the orchestrator's. Held commands go "in the order printed".
- Step 10: the staging read uses the same drift call; the rule of staging only listed candidates is unchanged; the flow-store commit's own `git status --porcelain` over `.claude/flows/<slug>/` etc. stays. Stamping on re-entry uses the drift call too.
- New-tasks paragraph: one clause says the gate's checks come from `plan emit gate-list`.

The links to `findings.md` are unchanged. A milestone is still exactly two commits, and nothing commits, moves or discards an unowned edit.

## Deviations

- The suggested means said the file never says "waves", and it does not. No pool wording was needed.
- Re-entry stamping: `git show --name-only` became `git -c core.quotePath=false show --name-only`. That read is also compared with rows' `files`, and by default it C-quotes non-ASCII paths. The detail named only the `git status` reads, but the outcome covers every path comparison the drain makes. The alternative was `-z`, which is exact but hard to read in prose. Paths holding `"`, `\` or control characters are still quoted under `quotePath=false`.
- A widening holder in a later milestone sends the path to the user instead of adding `needs`. The required means does not cover this case. Without it, the current milestone would wait behind a later one.

## Handover

None.

## Checks

- Compile check: not applicable, since no code was edited.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 55 passed.
- Accept checks: all four `Run by: worker` greps pass (`plan emit gate-list`, `agents audit --transcripts`, `needs_files`, no `git status --porcelain -- <paths?>`).
- I checked that `plan drift --uncommitted --get uncommitted_files` prints one unquoted path per line against this flow (it reads `--porcelain=v1 -z --no-renames --untracked-files=all` in `tomlctl/src/plan/drift.rs`). I checked the audit's `{agent, command}` array shape in `tomlctl/src/agents/audit.rs`.

TANGENTIAL: question | claude/skills/implement-v2-engine/references/run-milestone.md | the drain's audit expects the launch step to keep the `Transcript dir:` path in `<scratch>`, but no file name is fixed between the two references | if the two tasks name the location differently, the orchestrator has to guess where the directory was kept
