Status: done — the engine has its findings reference with the reconcile step, linked from the step table, drain.md and the carrier's pre-flight

## What changed

- `claude/skills/implement-v2-engine/references/findings.md` (new): what an apply row is (a row a `[[finding_links]]` entry names, with `store`, `fixes`, `settles`) and how to list a milestone's apply rows and linked stores; the store-path to `--slug`/`--scope` mapping; what the worker sees (the worker view's `## Findings` section, read through the implementer projection); anchor premises on a started row (a `fails` anchor premise on a row whose `agent` is a plan worker does not withhold it; on an unstarted row it withholds; `broken` withholds either way); the exemption of linked review stores outside the flow directory, with sidecars, from the pre-flight clean-tree check and the drain's out-of-lane rule; and the Reconcile section (`tomlctl review reconcile --slug|--scope ... --flow <slug>` per distinct linked store, after the stamp and before the flow-store commit, with staging of an outside store and its sidecar in the flow-store commit, so commits stay two per milestone).
- `claude/skills/implement-v2-engine/SKILL.md`: step table gains a "Linked review stores" row and a "Reconcile" row, and the Premises row points to the anchor rule; the reference list gains findings.md; the out-of-scope line now reads "Vetting and harvest" and says apply rows are in scope; "the two references" became "its references".
- `claude/skills/implement-v2-engine/references/drain.md`: two pointers only. Step 3's out-of-lane list names linked review stores by pointer to findings.md; step 10 points to findings.md's Reconcile after `backlog reconcile`.
- `claude/commands/implement-v2.md`: pre-flight feature list gains `tasks_finding_links`, `review_schedule`, `review_reconcile`, `review_anchor`, `review_reproduce`, `review_stats_apply`.

## Deviations

- None from a `suggested` means (the task has none). Choices made to fill the detail: the anchor rule names `broken` as still withholding, since only a `fails` verdict is explained by the row's own edit; a reconcile refusal stops before the flow-store commit and goes to `needs_human`, mirroring a failed commit. The reconcile command is written as two lines (`--slug` / `--scope`) rather than a `<store flag>` placeholder, because the in-crate command lint parses bash fences and a spaced placeholder would read as a positional that `review reconcile` does not take.
- run-milestone.md (pre-flight clean-tree check, premise step) is not in this task's files, so the exemption and the anchor rule live in findings.md and are reached from the SKILL.md step table, not from run-milestone.md itself. A later task holding run-milestone.md may want a pointer there.

## Checks

- Compile check: not applicable (markdown only). Verified every new `tomlctl` invocation against the source CLI definitions (`review reconcile` takes `--slug`/`--scope` and `--flow`) and ran the read verbs against this flow's store: `items list --array finding_links --where-in/--select/--pluck --distinct --raw --lines` and `tasks list --pluck ref --raw --lines` all exit 0.
- Accept 1 (`grep -q 'review reconcile' .../findings.md`): pass.
- Accept 2 (`grep -q 'references/findings.md' .../SKILL.md`): pass.
- Accept 3 (`grep -q 'review_reconcile' claude/commands/implement-v2.md`): pass.
- Not run: `cli::dispatch::tests` command lint (builds the crate); the gate runs it.

## Handover

None proposed. Note for the orchestrator: the installed `tomlctl` on this machine lacks all six new features (`review reconcile` is an unrecognised subcommand), so once this lands the carrier's pre-flight refuses until `cargo install --path tomlctl` is rerun from the tree that has them.

TANGENTIAL: question | claude/skills/implement-v2-engine/references/run-milestone.md | the clean-tree check (step 1) and premise verdict table (step 3) do not point to findings.md's linked-store exemption and anchor rule | an orchestrator reading only run-milestone.md would stop on a dirty linked store or withhold a started apply row
