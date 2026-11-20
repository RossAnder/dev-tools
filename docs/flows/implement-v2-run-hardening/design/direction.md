# Direction

## Overarching

Every step of a v2 milestone run that the orchestrator did by eye or by hand becomes a `tomlctl` verb, field or check, and the engine prose is rewritten to call it. The plan optimises for a run that is mechanical and auditable; it gives up a smaller binary surface and accepts two schema stamps and a reinstall.

Expectations every task stays consistent with:

- Follow the precedent already in the crate rather than inventing a shape. A new `plan emit` leaf is built like `lane-check`; a list that names gates by command string is built like `gate_probes`; a new plan-store array is built like the run records; a new optional key is written only when set.
- A flag or verb and its flag table land together. A task that adds a flag, a leaf or a check class updates the `tomlctl` skill reference that documents it in the same task, because the dispatch lint compares the two.
- Prose says what the orchestrator runs and reads, as a command and the key it reads. It never restates what the verb computes.
- Nothing changes for a store that uses none of the new keys: no new stamp, no new output key, no new finding, and the drain still holds, by judgement, an unmarked gate command that installs, links or deploys; `held` adds to that judgement and never replaces it.
- The `verification` agent, the v1 carriers and `plan drift`'s output stay as they are.
- A fenced `tomlctl` command in a skill, command or agent file takes single-token placeholders (`<store-flag>`, never `<store flag>`).

## Milestone A

Every `tomlctl` change, and nothing else. It delivers `plan emit commit-paths`, the external-effect mark with `gate-list`'s `held` key and its two checks, `plan run gate --blocks`, the audit's indirect-run pattern, `plan run visual --accept` with its plan-store record, the `plan/ui-accept-structural` exemption, and the feature strings, with each verb's reference in the `tomlctl` skill.

No task here edits the engine, the worker agents or the `/implement-v2` carrier: those files are live in the Claude config dir the moment they are written, and this milestone's own drain runs on the binary installed before it started. The milestone's gate ends by installing the new binary, which the orchestrator holds by judgement until the other gate commands pass, as the drain says today. A task here reads the direction as: build the verb so that the prose milestone B writes can be one command and one key.

Four tasks (1, 3, 6 and 11) add to `tomlctl/src/cli/types/plan.rs` and are chained by `needs` for their shared files; each keeps its own logic in files only it holds.

## Milestone B

Every prose change, written against the binary milestone A installed: the drain rewritten to use `commit-paths`, `held` and `--blocks`; the plan-authoring rules for the external mark; the engine prose that lets an accepted visual failure commit and be reported; the capture a visual fix task's worker runs for its own states; and the `/implement-v2` pre-flight's feature list.

Because the installed binary already has every verb, no sentence here carries a fallback for a binary that lacks one; the pre-flight refuses such a binary instead. A task here reads the direction as: make the drain's steps 3, 7 and 10 need no hand comparison and no retyping, and keep the orchestrator's judgement only where the plan carries no mark. For the visual pass, the inspector stays the only judge of record. The waiver records the user's decision and never a verdict of its own; the worker's capture is a self-check that changes no verdict and no round count.
