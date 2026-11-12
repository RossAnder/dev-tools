Verdict: a worker could carry out milestone C as written, with one false premise (task 37's base claim) and one unactionable point (task 36's report kinds), plus one accept check that cannot discriminate.

## Unactionable

1. **Task 36, Detail (3):** "add the `plan run report` call, one row per report line with its `kind`, `task`, `text` and `path`". The worker view never lists the values `kind` takes or how a report section maps to one. The only design section the view includes, `design/engine.md#drain-changes`, gives the command and nothing about entry kinds. The list exists in `docs/flows/glimpse-v2-visibility/design/records.md:90` (`parked | failed | handover | no-worker | withheld | gate | audit | out-of-lane | store-change | reconcile`), but the view neither includes nor cites it. A worker has to find that file on its own or read task 10's verb source. Otherwise it guesses kinds from the section headers of step 11's report shape. Fix: add `design/records.md`'s report section to task 36's design links, or name the kind list in the Detail.

## False premises

1. **Task 37, premise "the record lists topic 6 as agreed and not built (at base)"**, `grep -q "not planned or built" docs/ideas/v2-pipeline.md`, with "Probe: holds at `207a03a1ff85394de1138fcd1b9b8ff41d2aebc7`". It does not hold at that commit. `git show 207a03a1:docs/ideas/v2-pipeline.md | grep -c "not planned or built"` prints 0, and `grep -c "### Flow state and glimpse"` on the same blob also prints 0. At HEAD (= 207a03a1) topic 6's row reads `| 6 | Flow state and glimpse | Open |`, and the status line says "topics 4 to 8 remain open". The phrase, the "Agreed 2026-11-12" status line and the whole `### Flow state and glimpse` section (working tree lines 216-231) exist only as an uncommitted edit: `git status --porcelain -- docs/ideas/v2-pipeline.md` prints ` M`. The task's Detail edits that section ("replace the opening paragraph's statement that glimpse reads only the v1 row fields", "correct the three bullets"). If the edit is not committed before the run, a worker finds pre-existing uncommitted changes in its own file and cannot tell them from an earlier attempt's partial edit. If the edit is discarded, the section the task edits does not exist. The probe evidently ran against the working tree, not the named commit.

## Observations, not counted

- Task 36: the design's report command uses `--why '<why the invocation ended>'`, a multi-word placeholder, but the task's constraint says fenced `tomlctl` commands take single-token placeholders. The constraint is explicit, so a worker can follow it (for example `<why>`).
- Task 36: the flags of `plan run notes`, `plan run gate` and `plan run report` (`--round`, `--ndjson`, `--why`) could not be checked, because the verbs come from tasks 8, 9, 10 and 13, which are not on the tree yet. `cli::dispatch::tests` will hold the drain's fenced commands to them.
- Task 36, step 2 rewrite: today's step 2 checks the Status line against the index entry's `status` (`drain.md:47`). The new `problem` field compares it with the task row (design). A worker can carry the rule as the Detail says.

## Premise checks (present tree, HEAD 207a03a1)

- T34 `grep -q "SHARED-BLOCK:plan-worker-procedure END"` in both workers: holds (`plan-worker-deep.md:92`; `scripts/shared-blocks.toml:80-84` names exactly the two files).
- T34 `grep -q "^TANGENTIAL: <kind> | <area> | <summary> | <why it matters>" claude/agents/plan-worker-deep.md`: rc=0, holds (`plan-worker-deep.md:74`). The "The note" section is at `plan-worker-deep.md:60-79` and "Return" at :81, both as the task describes.
- T35 `SHARED-BLOCK:plan-v2-note-writer START` in the three plan agents: holds. `scripts/shared-blocks.toml:149-154` names exactly those three, and no other file under `claude/` carries the block.
- T36 "Two things are taken from every note…" at `drain.md:44`: rc=0. `^## 11. The .needs_human. report` at `drain.md:424`: rc=0. `plan_emit_lane_check` at `implement-v2.md:29`: holds. `plan_run_records` in `tomlctl/src/cli/types/mod.rs`: rc=1, as expected for an `after:13` premise.
- T36 step 8 reads `TANGENTIAL: flaky-test` from verification blocks (`drain.md:309`, `:338`), as the Detail says to keep.
- T37: see the false premise above.

## Check commands (present tree)

- T34 `grep -q "toml run-note" claude/agents/plan-worker-deep.md && grep -q "toml run-note" claude/agents/plan-worker-lite.md`: rc=1 (discriminates).
- T34 `! grep -q "^TANGENTIAL: <kind>" claude/agents/plan-worker-deep.md`: rc=1 (discriminates).
- T34/T35 `bash scripts/verify-shared-blocks.sh`: printed `shared-block parity: OK`, rc=0. It is a guard, so passing today is expected.
- T35 `grep -q "toml sources"` across the three plan agents: rc=1 (discriminates).
- T36 drain `tomlctl plan run notes|gate|report` greps: rc=1 (discriminates).
- T36 `grep -q "plan_run_records" claude/commands/implement-v2.md`: rc=1 (discriminates).
- T36 `grep -q "plan run" CLAUDE.md`: **rc=0 on the present tree. It cannot discriminate.** It already matches `CLAUDE.md:43` ("composes, settles and audits a plan run, one phase per…"). A stricter check such as `grep -q "plan run notes" CLAUDE.md` or a grep for `plan_run_records` would discriminate.
- T36 `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: not run (it compiles the crate, and the gate owns it).
- T37 `grep -q "docs/flows/glimpse-v2-visibility" docs/ideas/v2-pipeline.md`: rc=1 (discriminates).
- T37 `! grep -q "not planned or built" docs/ideas/v2-pipeline.md`: rc=1 on the working tree. It would pass at the committed base, where the phrase is absent, so it discriminates only while the uncommitted edit is present.

## Coverage

Read the worker views and files of tasks 34, 35, 36 and 37. Files read: `claude/agents/plan-worker-deep.md` (shared block and note section), `scripts/shared-blocks.toml`, `claude/skills/implement-v2-engine/references/drain.md` (steps 2-5 and 7-11), `claude/commands/implement-v2.md:29`, `CLAUDE.md` (Implement v2 deploy bullet), `docs/ideas/v2-pipeline.md` (status line, table, sections and diff against HEAD), and `docs/flows/glimpse-v2-visibility/design/records.md` (grep for report kinds only). Not checked: the flags and outputs of the `plan run` leaves, `run_note.rs` and `note_sources.rs` (none exist yet; they come from milestone A tasks), and whether `plan view --for digest` will expose attempts.
